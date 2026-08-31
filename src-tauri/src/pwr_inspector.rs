//! Bounded, read-only inspection of Wharf v1 patch operations.
//!
//! PWR uses little-endian magic, varint-delimited protobuf messages, and a
//! compressed stream containing the old/new TLC containers followed by sync
//! operations. This parser intentionally supports only the pieces needed for
//! diagnostics; failure never affects publication.

use flate2::read::GzDecoder;
use std::{
    fs::File,
    io::{self, BufReader, Read},
    path::Path,
};
use tauri::{AppHandle, Runtime};

use crate::debug::{self, DebugEventKind, DebugScope};

const PATCH_MAGIC: u32 = 0x0FEF_5F00;
const BLOCK_SIZE: u64 = 64 * 1024;
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_FILES: usize = 100_000;
const MAX_OPERATIONS: usize = 5_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SegmentKind {
    Reused,
    Fresh,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PatchFileAnalysis {
    pub path: String,
    pub size: u64,
    pub reused_bytes: u64,
    pub fresh_bytes: u64,
    pub segments: Vec<(SegmentKind, u64)>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PatchAnalysis {
    pub old_file_count: usize,
    pub new_file_count: usize,
    pub reused_bytes: u64,
    pub fresh_bytes: u64,
    pub operation_count: usize,
    pub files: Vec<PatchFileAnalysis>,
}

#[derive(Debug)]
struct ContainerFile {
    path: String,
    size: u64,
}

#[derive(Clone, Copy, Debug)]
enum WireValue<'a> {
    Varint(u64),
    Bytes(&'a [u8]),
    Fixed32,
    Fixed64,
}

struct WireCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> WireCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn next(&mut self) -> Result<Option<(u32, WireValue<'a>)>, String> {
        if self.offset == self.bytes.len() {
            return Ok(None);
        }
        let key = read_varint_slice(self.bytes, &mut self.offset)?;
        let number = u32::try_from(key >> 3).map_err(|_| "PWR field number overflow")?;
        if number == 0 {
            return Err("PWR contains field zero".into());
        }
        let value = match key & 7 {
            0 => WireValue::Varint(read_varint_slice(self.bytes, &mut self.offset)?),
            1 => {
                self.skip(8)?;
                WireValue::Fixed64
            }
            2 => {
                let length = usize::try_from(read_varint_slice(self.bytes, &mut self.offset)?)
                    .map_err(|_| "PWR field length overflow")?;
                let start = self.offset;
                self.skip(length)?;
                WireValue::Bytes(&self.bytes[start..start + length])
            }
            5 => {
                self.skip(4)?;
                WireValue::Fixed32
            }
            _ => return Err("PWR contains an unsupported protobuf wire type".into()),
        };
        Ok(Some((number, value)))
    }

    fn skip(&mut self, length: usize) -> Result<(), String> {
        self.offset = self
            .offset
            .checked_add(length)
            .filter(|offset| *offset <= self.bytes.len())
            .ok_or("PWR protobuf field exceeds its message")?;
        Ok(())
    }
}

pub(crate) fn inspect_patch(path: &Path) -> Result<PatchAnalysis, String> {
    let mut input = BufReader::new(
        File::open(path).map_err(|error| format!("could not open PWR patch: {error}"))?,
    );
    let mut magic = [0_u8; 4];
    input
        .read_exact(&mut magic)
        .map_err(|error| format!("could not read PWR magic: {error}"))?;
    if u32::from_le_bytes(magic) != PATCH_MAGIC {
        return Err("PWR patch has an incompatible magic value".into());
    }
    let header =
        read_delimited(&mut input)?.ok_or_else(|| "PWR patch is missing its header".to_string())?;
    let compression = compression_algorithm(&header)?;
    let mut stream: Box<dyn Read> = match compression {
        0 => Box::new(input),
        1 => Box::new(brotli::Decompressor::new(input, 64 * 1024)),
        2 => Box::new(GzDecoder::new(input)),
        3 => return Err("PWR Zstandard diagnostics are not supported by this build".into()),
        _ => return Err("PWR patch declares an unknown compression algorithm".into()),
    };
    let old_container = read_delimited(&mut stream)?
        .ok_or_else(|| "PWR patch is missing its old container".to_string())?;
    let new_container = read_delimited(&mut stream)?
        .ok_or_else(|| "PWR patch is missing its new container".to_string())?;
    let old_files = container_files(&old_container)?;
    let new_files = container_files(&new_container)?;
    let mut files = new_files
        .iter()
        .map(|file| PatchFileAnalysis {
            path: file.path.clone(),
            size: file.size,
            reused_bytes: 0,
            fresh_bytes: 0,
            segments: Vec::new(),
        })
        .collect::<Vec<_>>();
    let mut operation_count = 0_usize;

    while let Some(header) = read_delimited(&mut stream)? {
        let (sync_type, file_index) = sync_header(&header)?;
        if sync_type != 0 {
            return Err("PWR BSDIFF diagnostics are not supported".into());
        }
        let file_index = usize::try_from(file_index).map_err(|_| "PWR file index overflow")?;
        let file = files
            .get_mut(file_index)
            .ok_or("PWR sync header references an unknown target file")?;
        let mut produced = file.reused_bytes.saturating_add(file.fresh_bytes);
        loop {
            let operation = read_delimited(&mut stream)?
                .ok_or_else(|| "PWR patch ended inside a sync stream".to_string())?;
            let (kind, block_span, data_bytes) = sync_operation(&operation)?;
            if kind == 2049 {
                break;
            }
            operation_count = operation_count.saturating_add(1);
            if operation_count > MAX_OPERATIONS {
                return Err("PWR patch has too many operations for diagnostics".into());
            }
            let remaining = file.size.saturating_sub(produced);
            let (segment_kind, bytes) = match kind {
                0 => (
                    SegmentKind::Reused,
                    block_span.saturating_mul(BLOCK_SIZE).min(remaining),
                ),
                1 => (SegmentKind::Fresh, data_bytes.min(remaining)),
                _ => return Err("PWR patch contains an unknown sync operation".into()),
            };
            if bytes == 0 {
                continue;
            }
            produced = produced.saturating_add(bytes);
            match segment_kind {
                SegmentKind::Reused => file.reused_bytes = file.reused_bytes.saturating_add(bytes),
                SegmentKind::Fresh => file.fresh_bytes = file.fresh_bytes.saturating_add(bytes),
            }
            if file
                .segments
                .last()
                .is_some_and(|segment| segment.0 == segment_kind)
            {
                if let Some(segment) = file.segments.last_mut() {
                    segment.1 = segment.1.saturating_add(bytes);
                }
            } else {
                file.segments.push((segment_kind, bytes));
            }
        }
    }
    let reused_bytes = files.iter().map(|file| file.reused_bytes).sum();
    let fresh_bytes = files.iter().map(|file| file.fresh_bytes).sum();
    Ok(PatchAnalysis {
        old_file_count: old_files.len(),
        new_file_count: new_files.len(),
        reused_bytes,
        fresh_bytes,
        operation_count,
        files,
    })
}

pub(crate) fn emit_debug_analysis<R: Runtime>(app: &AppHandle<R>, path: &Path, phase: &str) {
    if !debug::runtime(app).is_enabled() {
        return;
    }
    match inspect_patch(path) {
        Ok(mut analysis) => {
            debug::event(
                app,
                DebugScope::Butler,
                DebugEventKind::Decision,
                Some(phase),
                Some("Decoded the PWR stream for an exact, read-only operation map."),
                None,
                [
                    ("old_files".into(), analysis.old_file_count.to_string()),
                    ("new_files".into(), analysis.new_file_count.to_string()),
                    ("operations".into(), analysis.operation_count.to_string()),
                    ("reused_bytes".into(), analysis.reused_bytes.to_string()),
                    ("fresh_bytes".into(), analysis.fresh_bytes.to_string()),
                ],
            );
            analysis
                .files
                .sort_by_key(|file| std::cmp::Reverse(file.size));
            for file in analysis.files.iter().filter(|file| file.size > 0).take(6) {
                let reused_percent = file
                    .reused_bytes
                    .saturating_mul(100)
                    .checked_div(file.size)
                    .unwrap_or(0);
                debug::event(
                    app,
                    DebugScope::Butler,
                    DebugEventKind::PatchMap,
                    Some(phase),
                    Some(&format!(
                        "{:<28.28} [{}] reuse {:>3}% | fresh {} bytes",
                        file.path,
                        file.block_map(24),
                        reused_percent,
                        file.fresh_bytes,
                    )),
                    None,
                    [],
                );
            }
        }
        Err(_) => debug::event(
            app,
            DebugScope::Butler,
            DebugEventKind::Warning,
            Some(phase),
            Some("The patch is valid, but its detailed operation map could not be decoded for diagnostics."),
            None,
            [("operation".into(), "continues normally".into())],
        ),
    }
}

impl PatchFileAnalysis {
    pub(crate) fn block_map(&self, width: usize) -> String {
        if width == 0 || self.size == 0 {
            return String::new();
        }
        let mut output = String::with_capacity(width);
        let mut segment_end = 0_u64;
        let mut segment_index = 0_usize;
        for cell in 0..width {
            let sample =
                ((cell as u128 * self.size as u128) + (self.size as u128 / 2)) / width as u128;
            let sample = u64::try_from(sample).unwrap_or(u64::MAX);
            while segment_index < self.segments.len()
                && sample >= segment_end.saturating_add(self.segments[segment_index].1)
            {
                segment_end = segment_end.saturating_add(self.segments[segment_index].1);
                segment_index += 1;
            }
            output.push(
                match self.segments.get(segment_index).map(|segment| segment.0) {
                    Some(SegmentKind::Reused) => 'R',
                    Some(SegmentKind::Fresh) => 'D',
                    None => '?',
                },
            );
        }
        output
    }
}

fn compression_algorithm(header: &[u8]) -> Result<u64, String> {
    let mut fields = WireCursor::new(header);
    while let Some((number, value)) = fields.next()? {
        if number == 1 {
            if let WireValue::Bytes(settings) = value {
                let mut settings = WireCursor::new(settings);
                while let Some((number, value)) = settings.next()? {
                    if number == 1 {
                        if let WireValue::Varint(algorithm) = value {
                            return Ok(algorithm);
                        }
                    }
                }
            }
        }
    }
    Ok(0)
}

fn container_files(container: &[u8]) -> Result<Vec<ContainerFile>, String> {
    let mut output = Vec::new();
    let mut fields = WireCursor::new(container);
    while let Some((number, value)) = fields.next()? {
        if number != 1 {
            continue;
        }
        let WireValue::Bytes(file) = value else {
            return Err("PWR container file has the wrong wire type".into());
        };
        if output.len() == MAX_FILES {
            return Err("PWR container has too many files for diagnostics".into());
        }
        let mut path = None;
        let mut size = 0_u64;
        let mut file_fields = WireCursor::new(file);
        while let Some((number, value)) = file_fields.next()? {
            match (number, value) {
                (1, WireValue::Bytes(bytes)) => {
                    path = Some(
                        std::str::from_utf8(bytes)
                            .map_err(|_| "PWR file path is not UTF-8")?
                            .to_string(),
                    );
                }
                (3, WireValue::Varint(value)) => size = value,
                _ => {}
            }
        }
        output.push(ContainerFile {
            path: path.ok_or("PWR container file is missing its path")?,
            size,
        });
    }
    Ok(output)
}

fn sync_header(message: &[u8]) -> Result<(u64, u64), String> {
    let mut sync_type = 0;
    let mut file_index = None;
    let mut fields = WireCursor::new(message);
    while let Some((number, value)) = fields.next()? {
        match (number, value) {
            (1, WireValue::Varint(value)) => sync_type = value,
            (16, WireValue::Varint(value)) => file_index = Some(value),
            _ => {}
        }
    }
    // Protobuf omits scalar zero values. The first file therefore has no
    // encoded field 16 in canonical Wharf output.
    Ok((sync_type, file_index.unwrap_or(0)))
}

fn sync_operation(message: &[u8]) -> Result<(u64, u64, u64), String> {
    let mut kind = 0;
    let mut span = 0;
    let mut data_bytes = 0;
    let mut fields = WireCursor::new(message);
    while let Some((number, value)) = fields.next()? {
        match (number, value) {
            (1, WireValue::Varint(value)) => kind = value,
            (4, WireValue::Varint(value)) => span = value,
            (5, WireValue::Bytes(value)) => data_bytes = value.len() as u64,
            _ => {}
        }
    }
    Ok((kind, span, data_bytes))
}

fn read_delimited(reader: &mut dyn Read) -> Result<Option<Vec<u8>>, String> {
    let Some(length) = read_varint_reader(reader)
        .map_err(|error| format!("could not read PWR message length: {error}"))?
    else {
        return Ok(None);
    };
    let length = usize::try_from(length).map_err(|_| "PWR message length overflow")?;
    if length > MAX_MESSAGE_BYTES {
        return Err("PWR message is too large for bounded diagnostics".into());
    }
    let mut message = vec![0; length];
    reader
        .read_exact(&mut message)
        .map_err(|error| format!("could not read PWR message: {error}"))?;
    Ok(Some(message))
}

fn read_varint_reader(reader: &mut dyn Read) -> io::Result<Option<u64>> {
    let mut output = 0_u64;
    for shift in (0..70).step_by(7) {
        let mut byte = [0_u8; 1];
        match reader.read_exact(&mut byte) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof && shift == 0 => {
                return Ok(None);
            }
            Err(error) => return Err(error),
        }
        output |= u64::from(byte[0] & 0x7f) << shift;
        if byte[0] & 0x80 == 0 {
            return Ok(Some(output));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "protobuf varint is too long",
    ))
}

fn read_varint_slice(bytes: &[u8], offset: &mut usize) -> Result<u64, String> {
    let mut output = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.get(*offset).ok_or("truncated protobuf varint")?;
        *offset += 1;
        output |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(output);
        }
    }
    Err("protobuf varint is too long".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_operation_order_as_a_fixed_width_map() {
        let file = PatchFileAnalysis {
            path: "game.pak".into(),
            size: 100,
            reused_bytes: 75,
            fresh_bytes: 25,
            segments: vec![
                (SegmentKind::Reused, 50),
                (SegmentKind::Fresh, 25),
                (SegmentKind::Reused, 25),
            ],
        };
        assert_eq!(file.block_map(8), "RRRRDDRR");
    }

    #[test]
    fn parses_small_protobuf_fields_without_generated_code() {
        let message = [0x08, 0x02, 0x2a, 0x03, b'a', b'b', b'c'];
        let mut fields = WireCursor::new(&message);
        assert!(matches!(
            fields.next().unwrap(),
            Some((1, WireValue::Varint(2)))
        ));
        assert!(matches!(
            fields.next().unwrap(),
            Some((5, WireValue::Bytes(b"abc")))
        ));
        assert!(fields.next().unwrap().is_none());
    }
}
