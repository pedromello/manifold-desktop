//! Opt-in, read-only diagnostics for the distribution pipeline.
//!
//! Set `MANIFOLD_DEBUG_CONSOLE=1` before starting Manifold Desktop. The app
//! launches a companion console and streams sanitized, versioned NDJSON events
//! to it over a token-authenticated loopback socket. Diagnostics are deliberately
//! best-effort: a slow, closed, or broken console must never affect a release or
//! an installation.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    fs::{self, File},
    io::{BufRead, BufReader, BufWriter, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    process::Command,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager, Runtime};

const CHILD_ARGUMENT: &str = "--manifold-debug-console-child";
const DEBUG_ENVIRONMENT_VARIABLE: &str = "MANIFOLD_DEBUG_CONSOLE";
const EVENT_SCHEMA_VERSION: u8 = 1;
const EVENT_QUEUE_CAPACITY: usize = 512;
const MAX_RECENT_MESSAGES: usize = 8;
const CONSOLE_WIDTH: usize = 108;
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_FAINT: &str = "\x1b[2m";
const BRAND_VIOLET: (u8, u8, u8) = (105, 61, 255);
const BRAND_MAGENTA: (u8, u8, u8) = (255, 43, 178);
const BRAND_TOP: (u8, u8, u8) = (224, 39, 246);
const BRAND_BOTTOM: (u8, u8, u8) = (105, 54, 238);
const PREFERRED_CONSOLE_COLUMNS: usize = 120;
const PREFERRED_CONSOLE_ROWS: usize = 40;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DebugScope {
    System,
    Publisher,
    Api,
    Butler,
    Updater,
    Downloader,
    Verifier,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DebugEventKind {
    Session,
    Stage,
    Progress,
    Message,
    ApiCall,
    Decision,
    PatchMap,
    Complete,
    Warning,
    Error,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct DebugEvent {
    pub schema_version: u8,
    pub sequence: u64,
    pub timestamp_ms: u128,
    pub session_id: String,
    pub scope: DebugScope,
    pub kind: DebugEventKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct DebugRuntime {
    inner: Option<Arc<DebugRuntimeInner>>,
}

#[derive(Debug)]
struct DebugRuntimeInner {
    session_id: String,
    sequence: AtomicU64,
    sender: SyncSender<DebugEvent>,
}

impl DebugRuntime {
    pub fn from_environment() -> Self {
        if !debug_requested() {
            return Self::default();
        }
        match start_debug_session() {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("Manifold debug console could not start: {error}");
                Self::default()
            }
        }
    }

    #[cfg(test)]
    fn for_sender(session_id: &str, sender: SyncSender<DebugEvent>) -> Self {
        Self {
            inner: Some(Arc::new(DebugRuntimeInner {
                session_id: session_id.into(),
                sequence: AtomicU64::new(0),
                sender,
            })),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub fn emit(
        &self,
        scope: DebugScope,
        kind: DebugEventKind,
        phase: Option<&str>,
        message: Option<&str>,
        progress: Option<(u64, u64, &str)>,
        fields: impl IntoIterator<Item = (String, String)>,
    ) {
        let Some(inner) = &self.inner else {
            return;
        };
        let (current, total, unit) = progress
            .map(|(current, total, unit)| (Some(current), Some(total), Some(unit.to_string())))
            .unwrap_or((None, None, None));
        let event = DebugEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            sequence: inner.sequence.fetch_add(1, Ordering::Relaxed) + 1,
            timestamp_ms: now_millis(),
            session_id: inner.session_id.clone(),
            scope,
            kind,
            phase: phase.map(sanitize_text),
            message: message.map(sanitize_text),
            current,
            total,
            unit,
            fields: fields
                .into_iter()
                .map(|(key, value)| (sanitize_key(&key), sanitize_field(&key, &value)))
                .collect(),
        };
        // Never wait for diagnostics. Dropping an event under pressure is safer
        // than delaying a release, a game update, or the Tauri event loop.
        let _ = inner.sender.try_send(event);
    }

    pub fn stage(&self, scope: DebugScope, phase: &str, message: &str) {
        self.emit(
            scope,
            DebugEventKind::Stage,
            Some(phase),
            Some(message),
            None,
            [],
        );
    }

    pub fn progress(&self, scope: DebugScope, phase: &str, current: u64, total: u64, unit: &str) {
        self.emit(
            scope,
            DebugEventKind::Progress,
            Some(phase),
            None,
            Some((current, total, unit)),
            [],
        );
    }
}

pub fn runtime<R: Runtime>(app: &AppHandle<R>) -> DebugRuntime {
    app.try_state::<DebugRuntime>()
        .map(|state| state.inner().clone())
        .unwrap_or_default()
}

pub fn stage<R: Runtime>(app: &AppHandle<R>, scope: DebugScope, phase: &str, message: &str) {
    runtime(app).stage(scope, phase, message);
}

pub fn progress<R: Runtime>(
    app: &AppHandle<R>,
    scope: DebugScope,
    phase: &str,
    current: u64,
    total: u64,
    unit: &str,
) {
    runtime(app).progress(scope, phase, current, total, unit);
}

pub fn event<R: Runtime>(
    app: &AppHandle<R>,
    scope: DebugScope,
    kind: DebugEventKind,
    phase: Option<&str>,
    message: Option<&str>,
    progress: Option<(u64, u64, &str)>,
    fields: impl IntoIterator<Item = (String, String)>,
) {
    runtime(app).emit(scope, kind, phase, message, progress, fields);
}

fn debug_requested() -> bool {
    std::env::var(DEBUG_ENVIRONMENT_VARIABLE)
        .ok()
        .is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn session_identity() -> (String, String) {
    let seed = format!(
        "{}:{}:{:?}",
        std::process::id(),
        now_millis(),
        thread::current().id()
    );
    let digest = format!("{:x}", Sha256::digest(seed.as_bytes()));
    (format!("dbg-{}", &digest[..12]), digest)
}

fn start_debug_session() -> Result<DebugRuntime, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("could not bind loopback diagnostics socket: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("could not inspect diagnostics socket: {error}"))?;
    let (session_id, token) = session_identity();
    let (sender, receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
    let session_path = session_log_path(&session_id);
    let writer_token = token.clone();
    thread::Builder::new()
        .name("manifold-debug-writer".into())
        .spawn(move || {
            let mut recording = open_recording(session_path).ok();
            let mut console = authenticated_console(listener, &writer_token).ok();
            while let Ok(event) = receiver.recv() {
                let Ok(line) = serde_json::to_string(&event) else {
                    continue;
                };
                if let Some(file) = recording.as_mut() {
                    if writeln!(file, "{line}")
                        .and_then(|()| file.flush())
                        .is_err()
                    {
                        recording = None;
                    }
                }
                if let Some(stream) = console.as_mut() {
                    if writeln!(stream, "{line}")
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        console = None;
                    }
                }
            }
        })
        .map_err(|error| format!("could not start diagnostics writer: {error}"))?;

    launch_console(address.to_string(), token, session_id.clone())?;
    let runtime = DebugRuntime {
        inner: Some(Arc::new(DebugRuntimeInner {
            session_id: session_id.clone(),
            sequence: AtomicU64::new(0),
            sender,
        })),
    };
    runtime.emit(
        DebugScope::System,
        DebugEventKind::Session,
        Some("ready"),
        Some("Debug console connected. Waiting for a publish or update operation."),
        None,
        [("mode".into(), "read-only".into())],
    );
    start_demo_if_requested(runtime.clone());
    Ok(runtime)
}

fn start_demo_if_requested(runtime: DebugRuntime) {
    let Ok(mode) = std::env::var("MANIFOLD_DEBUG_DEMO") else {
        return;
    };
    let mode = mode.trim().to_ascii_lowercase();
    if !matches!(mode.as_str(), "publisher" | "updater") {
        return;
    }
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(500));
        if mode == "publisher" {
            runtime.stage(
                DebugScope::Publisher,
                "preparing_patch",
                "Demo: comparing release 1.4.0 with 1.5.0 using 64 KiB Wharf blocks.",
            );
            thread::sleep(Duration::from_millis(450));
            runtime.emit(
                DebugScope::Butler,
                DebugEventKind::Decision,
                Some("patch_analysis"),
                Some("Unchanged data will be reused locally; only fresh DATA operations enter the .pwr payload."),
                None,
                [
                    ("reused_data".into(), "92.4%".into()),
                    ("fresh_data".into(), "7.6%".into()),
                    ("patch_size".into(), "38.2 MiB".into()),
                    ("full_archive".into(), "502.0 MiB".into()),
                ],
            );
            for row in [
                "game.pak                     [RRRRRRRRRRRRRRRRRRDDDDRR] reuse  83% | fresh 8912896 bytes",
                "bin/Game.exe                 [RRRRRRRRRRRRRRRDDDDDDDDD] reuse  63% | fresh 3145728 bytes",
                "data/new-level.bundle       [DDDDDDDDDDDDDDDDDDDDDDDD] reuse   0% | fresh 28311552 bytes",
            ] {
                runtime.emit(
                    DebugScope::Butler,
                    DebugEventKind::PatchMap,
                    Some("patch_operations"),
                    Some(row),
                    None,
                    [],
                );
            }
            for percentage in [8, 24, 43, 67, 86, 100] {
                thread::sleep(Duration::from_millis(180));
                runtime.progress(
                    DebugScope::Publisher,
                    "uploading_patch",
                    percentage,
                    100,
                    "percent",
                );
            }
            runtime.emit(
                DebugScope::Api,
                DebugEventKind::Complete,
                Some("confirm_patch"),
                Some(
                    "Demo complete: patch and signature are READY; the full ZIP remains available.",
                ),
                Some((100, 100, "percent")),
                [("status".into(), "READY".into())],
            );
        } else {
            runtime.emit(
                DebugScope::Api,
                DebugEventKind::Decision,
                Some("resolve_update"),
                Some("Demo: the API selected PATCH from 1.4.0 to 1.5.0."),
                None,
                [
                    ("strategy".into(), "PATCH".into()),
                    ("download_saved".into(), "463.8 MiB".into()),
                    ("full_fallback".into(), "available".into()),
                ],
            );
            for row in [
                "game.pak                     [RRRRRRRRRRRRRRRRRRDDDDRR] reuse  83% | fresh 8912896 bytes",
                "bin/Game.exe                 [RRRRRRRRRRRRRRRDDDDDDDDD] reuse  63% | fresh 3145728 bytes",
                "data/new-level.bundle       [DDDDDDDDDDDDDDDDDDDDDDDD] reuse   0% | fresh 28311552 bytes",
            ] {
                runtime.emit(
                    DebugScope::Butler,
                    DebugEventKind::PatchMap,
                    Some("downloaded_patch_operations"),
                    Some(row),
                    None,
                    [],
                );
            }
            for percentage in [4, 19, 41, 64, 83, 100] {
                thread::sleep(Duration::from_millis(180));
                runtime.progress(
                    DebugScope::Downloader,
                    "downloading_update",
                    percentage,
                    100,
                    "percent",
                );
            }
            runtime.stage(
                DebugScope::Updater,
                "applying_update",
                "Demo: reconstructing the target in isolated staging while the installed game stays playable.",
            );
            thread::sleep(Duration::from_millis(550));
            runtime.emit(
                DebugScope::Verifier,
                DebugEventKind::Complete,
                Some("verifying_update"),
                Some("Demo complete: staging matches the canonical signature and is ready for atomic activation."),
                Some((100, 100, "percent")),
                [("active_installation".into(), "untouched until activation".into())],
            );
        }
    });
}

fn session_log_path(session_id: &str) -> PathBuf {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Manifold")
        .join("debug-sessions");
    root.join(format!("{session_id}.jsonl"))
}

fn open_recording(path: PathBuf) -> std::io::Result<BufWriter<File>> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    File::create(path).map(BufWriter::new)
}

fn authenticated_console(listener: TcpListener, expected_token: &str) -> Result<TcpStream, String> {
    let (stream, address) = listener
        .accept()
        .map_err(|error| format!("debug console did not connect: {error}"))?;
    if !address.ip().is_loopback() {
        return Err("debug console connection was not local".into());
    }
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|error| format!("could not configure debug handshake: {error}"))?;
    let mut token = String::new();
    BufReader::new(
        stream
            .try_clone()
            .map_err(|error| format!("could not clone debug socket: {error}"))?,
    )
    .read_line(&mut token)
    .map_err(|error| format!("could not read debug handshake: {error}"))?;
    if token.trim_end() != expected_token {
        return Err("debug console authentication failed".into());
    }
    let _ = stream.set_read_timeout(None);
    Ok(stream)
}

fn launch_console(address: String, token: String, session_id: String) -> Result<(), String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("could not locate Manifold Desktop: {error}"))?;
    let mut command = Command::new(executable);
    command.args([CHILD_ARGUMENT, &address, &token, &session_id]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("could not open companion console: {error}"))
}

/// Runs the internal companion-console mode and returns true when the normal
/// Tauri process must not be started.
pub fn maybe_run_console_child() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let Some(position) = args.iter().position(|argument| argument == CHILD_ARGUMENT) else {
        return false;
    };
    if args.len() < position + 4 {
        eprintln!("Invalid Manifold debug console invocation.");
        return true;
    }
    if let Err(error) = run_console(
        &args[position + 1],
        &args[position + 2],
        &args[position + 3],
    ) {
        eprintln!("Manifold debug console stopped: {error}");
    }
    true
}

fn run_console(address: &str, token: &str, session_id: &str) -> Result<(), String> {
    prepare_console_io();
    request_console_viewport();
    set_console_title();
    let mut stream = TcpStream::connect(address)
        .map_err(|error| format!("could not connect to the desktop process: {error}"))?;
    writeln!(stream, "{token}")
        .and_then(|()| stream.flush())
        .map_err(|error| format!("could not authenticate the debug console: {error}"))?;
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let mut view = ConsoleView::new(session_id, no_color);
    view.render();
    for line in BufReader::new(stream).lines() {
        let line = line.map_err(|error| format!("debug stream ended unexpectedly: {error}"))?;
        match serde_json::from_str::<DebugEvent>(&line) {
            Ok(event) => {
                view.accept(event);
                view.render();
            }
            Err(_) => view.push_message("Ignored an incompatible diagnostic event.".into()),
        }
    }
    view.push_message("Desktop process disconnected. Press Enter to close.".into());
    view.render();
    let _ = std::io::stdin().read_line(&mut String::new());
    Ok(())
}

#[cfg(windows)]
fn prepare_console_io() {
    use std::{ffi::c_void, os::windows::ffi::OsStrExt, ptr};

    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn CreateFileW(
            file_name: *const u16,
            desired_access: u32,
            share_mode: u32,
            security_attributes: *mut c_void,
            creation_disposition: u32,
            flags_and_attributes: u32,
            template_file: *mut c_void,
        ) -> *mut c_void;
        fn SetStdHandle(kind: u32, handle: *mut c_void) -> i32;
        fn GetConsoleMode(handle: *mut c_void, mode: *mut u32) -> i32;
        fn SetConsoleMode(handle: *mut c_void, mode: u32) -> i32;
    }

    const GENERIC_READ: u32 = 0x8000_0000;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const OPEN_EXISTING: u32 = 3;
    const STD_INPUT_HANDLE: u32 = -10_i32 as u32;
    const STD_OUTPUT_HANDLE: u32 = -11_i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12_i32 as u32;
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0000_0004;

    let wide = |value: &str| {
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>()
    };
    let input_name = wide("CONIN$");
    let output_name = wide("CONOUT$");
    // SAFETY: the names are live NUL-terminated UTF-16 buffers, all optional
    // pointer parameters are null as documented, and handles live until this
    // short-lived companion process exits.
    unsafe {
        let input = CreateFileW(
            input_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        let output = CreateFileW(
            output_name.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        );
        let _ = SetStdHandle(STD_INPUT_HANDLE, input);
        let _ = SetStdHandle(STD_OUTPUT_HANDLE, output);
        let _ = SetStdHandle(STD_ERROR_HANDLE, output);
        let mut mode = 0;
        if GetConsoleMode(output, &mut mode) != 0 {
            let _ = SetConsoleMode(output, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING);
        }
    }
}

#[cfg(not(windows))]
fn prepare_console_io() {}

#[cfg(windows)]
fn set_console_title() {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn SetConsoleTitleW(title: *const u16) -> i32;
    }
    let title: Vec<u16> = std::ffi::OsStr::new("Manifold Incremental Debug Console")
        .encode_wide()
        .chain(Some(0))
        .collect();
    // SAFETY: `title` is a live, NUL-terminated UTF-16 buffer for the duration
    // of this Windows API call. Failure only leaves the default console title.
    unsafe {
        let _ = SetConsoleTitleW(title.as_ptr());
    }
}

#[cfg(not(windows))]
fn set_console_title() {
    print!("\x1b]0;Manifold Incremental Debug Console\x07");
}

fn request_console_viewport() {
    console_write(&format!(
        "\x1b[8;{PREFERRED_CONSOLE_ROWS};{PREFERRED_CONSOLE_COLUMNS}t"
    ));
}

struct ConsoleView {
    session_id: String,
    no_color: bool,
    scope: DebugScope,
    phase: String,
    current: u64,
    total: u64,
    unit: String,
    fields: BTreeMap<String, String>,
    patch_rows: VecDeque<String>,
    messages: VecDeque<String>,
}

impl ConsoleView {
    fn new(session_id: &str, no_color: bool) -> Self {
        Self {
            session_id: session_id.into(),
            no_color,
            scope: DebugScope::System,
            phase: "starting".into(),
            current: 0,
            total: 0,
            unit: "events".into(),
            fields: BTreeMap::new(),
            patch_rows: VecDeque::new(),
            messages: VecDeque::new(),
        }
    }

    fn accept(&mut self, event: DebugEvent) {
        self.scope = event.scope;
        if let Some(phase) = event.phase {
            self.phase = phase;
        }
        if let (Some(current), Some(total)) = (event.current, event.total) {
            self.current = current;
            self.total = total;
            self.unit = event.unit.unwrap_or_else(|| "items".into());
        }
        if let Some(message) = event.message {
            if event.kind == DebugEventKind::PatchMap {
                if self.patch_rows.len() == 6 {
                    self.patch_rows.pop_front();
                }
                self.patch_rows.push_back(message);
            } else {
                self.push_message(message);
            }
        }
        for (key, value) in event.fields {
            self.fields.insert(key, value);
        }
    }

    fn push_message(&mut self, message: String) {
        if self.messages.len() == MAX_RECENT_MESSAGES {
            self.messages.pop_front();
        }
        self.messages.push_back(message);
    }

    fn render(&self) {
        use std::fmt::Write as _;
        let mut output = String::from("\x1b[2J\x1b[H");
        let faint = if self.no_color { "" } else { ANSI_FAINT };
        let reset = if self.no_color { "" } else { ANSI_RESET };
        let _ = writeln!(
            output,
            "{faint}Read-only diagnostics. Signed URLs, credentials, and local paths are redacted.{reset}"
        );
        let _ = write!(output, "{}", brand_header(&self.session_id, self.no_color));
        let _ = writeln!(output, "{}", separator(self.no_color));
        let _ = writeln!(output, "Scope  : {:?}", self.scope);
        let _ = writeln!(output, "Stage  : {}", self.phase);
        let _ = writeln!(
            output,
            "Progress: {}  {} / {} {}",
            colored_progress_bar(self.current, self.total, 36, self.no_color),
            self.current,
            self.total,
            self.unit
        );
        if !self.fields.is_empty() {
            let _ = writeln!(output, "{}", separator(self.no_color));
            for (key, value) in self.fields.iter().rev().take(6).rev() {
                let _ = writeln!(output, "{key:>22}: {value}");
            }
        }
        if !self.patch_rows.is_empty() {
            let _ = writeln!(output, "{}", separator(self.no_color));
            let _ = writeln!(
                output,
                "Wharf block map  (R = reused locally, D = fresh .pwr data)"
            );
            for row in &self.patch_rows {
                let _ = writeln!(output, "  {}", colorize_patch_row(row, self.no_color));
            }
        }
        let _ = writeln!(output, "{}", separator(self.no_color));
        let _ = writeln!(output, "Recent events");
        for message in &self.messages {
            let _ = writeln!(output, "  • {message}");
        }
        let _ = writeln!(output);
        console_write(&output);
    }
}

const LOGO_WIDTH: usize = 22;
const LOGO_HEIGHT: usize = 18;
const TERMINAL_BACKGROUND: (u8, u8, u8) = (8, 8, 10);

#[derive(Clone, Copy)]
struct LogoPixel {
    color: (u8, u8, u8),
    is_mark: bool,
}

fn manifold_logo_row(row: usize, no_color: bool) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    for column in 0..LOGO_WIDTH {
        let top = manifold_logo_pixel(column, row * 2);
        let bottom = manifold_logo_pixel(column, row * 2 + 1);
        if no_color {
            output.push(match (top, bottom) {
                (Some(top), Some(bottom)) if top.is_mark || bottom.is_mark => '█',
                (Some(_), Some(_)) => '▓',
                (Some(_), None) => '▀',
                (None, Some(_)) => '▄',
                (None, None) => ' ',
            });
        } else {
            let (top_red, top_green, top_blue) =
                top.map(|pixel| pixel.color).unwrap_or(TERMINAL_BACKGROUND);
            let (bottom_red, bottom_green, bottom_blue) = bottom
                .map(|pixel| pixel.color)
                .unwrap_or(TERMINAL_BACKGROUND);
            let _ = write!(
                output,
                "\x1b[38;2;{top_red};{top_green};{top_blue}m\x1b[48;2;{bottom_red};{bottom_green};{bottom_blue}m▀"
            );
        }
    }
    if !no_color {
        output.push_str(ANSI_RESET);
    }
    output
}

fn manifold_logo_pixel(column: usize, row: usize) -> Option<LogoPixel> {
    let x = column as f32 + 0.5;
    let y = row as f32 + 0.5;
    let inside = inside_logo_circle(x, y);
    let mark = inside && inside_logo_mark(x, y);
    if mark {
        return Some(LogoPixel {
            color: (252, 250, 255),
            is_mark: true,
        });
    }
    if inside {
        Some(LogoPixel {
            color: vertical_brand_color(row, LOGO_HEIGHT - 1),
            is_mark: false,
        })
    } else {
        None
    }
}

fn logo_circle_distance(x: f32, y: f32) -> f32 {
    let horizontal = (x - LOGO_WIDTH as f32 / 2.0) / 10.2;
    let vertical = (y - LOGO_HEIGHT as f32 / 2.0) / 8.3;
    horizontal * horizontal + vertical * vertical
}

fn inside_logo_circle(x: f32, y: f32) -> bool {
    logo_circle_distance(x, y) <= 1.0
}

fn inside_logo_mark(x: f32, y: f32) -> bool {
    const CURVES: [[(f32, f32); 4]; 4] = [
        [(5.7, 2.7), (8.0, 5.0), (10.2, 9.0), (10.7, 14.7)],
        [(16.3, 2.7), (14.0, 5.0), (11.8, 9.0), (11.3, 14.7)],
        [(2.7, 6.4), (5.2, 8.3), (6.8, 11.0), (7.0, 14.7)],
        [(19.3, 6.4), (16.8, 8.3), (15.2, 11.0), (15.0, 14.7)],
    ];
    CURVES.iter().any(|curve| {
        (0..=36).any(|step| {
            let t = step as f32 / 36.0;
            let inverse = 1.0 - t;
            let curve_x = inverse.powi(3) * curve[0].0
                + 3.0 * inverse.powi(2) * t * curve[1].0
                + 3.0 * inverse * t.powi(2) * curve[2].0
                + t.powi(3) * curve[3].0;
            let curve_y = inverse.powi(3) * curve[0].1
                + 3.0 * inverse.powi(2) * t * curve[1].1
                + 3.0 * inverse * t.powi(2) * curve[2].1
                + t.powi(3) * curve[3].1;
            (x - curve_x).powi(2) + (y - curve_y).powi(2) <= 0.78_f32.powi(2)
        })
    })
}

#[cfg(windows)]
fn console_write(value: &str) {
    use std::ffi::c_void;
    #[link(name = "Kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(kind: u32) -> *mut c_void;
        fn WriteConsoleW(
            output: *mut c_void,
            buffer: *const c_void,
            characters: u32,
            written: *mut u32,
            reserved: *mut c_void,
        ) -> i32;
    }
    const STD_OUTPUT_HANDLE: u32 = -11_i32 as u32;
    let wide: Vec<u16> = value.encode_utf16().collect();
    let mut written = 0;
    // SAFETY: `wide` is a live UTF-16 buffer and `written` is a valid output
    // pointer. The companion console is initialized before rendering.
    unsafe {
        let _ = WriteConsoleW(
            GetStdHandle(STD_OUTPUT_HANDLE),
            wide.as_ptr().cast(),
            u32::try_from(wide.len()).unwrap_or(u32::MAX),
            &mut written,
            std::ptr::null_mut(),
        );
    }
}

#[cfg(not(windows))]
fn console_write(value: &str) {
    print!("{value}");
    let _ = std::io::stdout().flush();
}

const WORDMARK_3D: [&str; 6] = [
    "███╗   ███╗ █████╗ ███╗   ██╗██╗███████╗ ██████╗ ██╗     ██████╗ ",
    "████╗ ████║██╔══██╗████╗  ██║██║██╔════╝██╔═══██╗██║     ██╔══██╗",
    "██╔████╔██║███████║██╔██╗ ██║██║█████╗  ██║   ██║██║     ██║  ██║",
    "██║╚██╔╝██║██╔══██║██║╚██╗██║██║██╔══╝  ██║   ██║██║     ██║  ██║",
    "██║ ╚═╝ ██║██║  ██║██║ ╚████║██║██║     ╚██████╔╝███████╗██████╔╝",
    "╚═╝     ╚═╝╚═╝  ╚═╝╚═╝  ╚═══╝╚═╝╚═╝      ╚═════╝ ╚══════╝╚═════╝ ",
];

fn brand_header(session_id: &str, no_color: bool) -> String {
    use std::fmt::Write as _;
    let mut output = String::new();
    for row in 0..(LOGO_HEIGHT / 2) {
        let _ = write!(output, "{} ", manifold_logo_row(row, no_color));
        if (2..(WORDMARK_3D.len() + 2)).contains(&row) {
            let _ = write!(output, "{}", wordmark_row(row - 2, no_color));
        } else if row == WORDMARK_3D.len() + 2 {
            let subtitle = format!("INCREMENTAL DISTRIBUTION DEBUG CONSOLE  |  {session_id}");
            if no_color {
                let _ = write!(output, "{subtitle}");
            } else {
                let _ = write!(output, "{ANSI_FAINT}{subtitle}{ANSI_RESET}");
            }
        }
        output.push('\n');
    }
    output
}

fn wordmark_row(row: usize, no_color: bool) -> String {
    use std::fmt::Write as _;
    let line = WORDMARK_3D[row];
    if no_color {
        return line.to_string();
    }
    let color = vertical_brand_color(row, WORDMARK_3D.len() - 1);
    let mut output = String::new();
    for character in line.chars() {
        if character == ' ' {
            output.push(character);
            continue;
        }
        let (red, green, blue) = if character == '█' {
            color
        } else {
            shade_color(color, 56)
        };
        let _ = write!(output, "\x1b[38;2;{red};{green};{blue}m{character}");
    }
    output.push_str(ANSI_RESET);
    output
}

fn gradient_color(position: usize, maximum: usize) -> (u8, u8, u8) {
    if maximum == 0 {
        return BRAND_VIOLET;
    }
    let interpolate = |start: u8, end: u8| {
        let start = i32::from(start);
        let delta = i32::from(end) - start;
        (start + delta * position as i32 / maximum as i32) as u8
    };
    (
        interpolate(BRAND_VIOLET.0, BRAND_MAGENTA.0),
        interpolate(BRAND_VIOLET.1, BRAND_MAGENTA.1),
        interpolate(BRAND_VIOLET.2, BRAND_MAGENTA.2),
    )
}

fn vertical_brand_color(position: usize, maximum: usize) -> (u8, u8, u8) {
    if maximum == 0 {
        return BRAND_TOP;
    }
    let interpolate = |start: u8, end: u8| {
        let start = i32::from(start);
        let delta = i32::from(end) - start;
        (start + delta * position as i32 / maximum as i32) as u8
    };
    (
        interpolate(BRAND_TOP.0, BRAND_BOTTOM.0),
        interpolate(BRAND_TOP.1, BRAND_BOTTOM.1),
        interpolate(BRAND_TOP.2, BRAND_BOTTOM.2),
    )
}

fn shade_color((red, green, blue): (u8, u8, u8), percentage: u16) -> (u8, u8, u8) {
    let shade = |channel: u8| ((u16::from(channel) * percentage) / 100) as u8;
    (shade(red), shade(green), shade(blue))
}

fn separator(no_color: bool) -> String {
    if no_color {
        "─".repeat(CONSOLE_WIDTH)
    } else {
        format!(
            "\x1b[38;2;{};{};{}m{}{}",
            BRAND_MAGENTA.0,
            BRAND_MAGENTA.1,
            BRAND_MAGENTA.2,
            "─".repeat(CONSOLE_WIDTH),
            ANSI_RESET
        )
    }
}

fn progress_bar(current: u64, total: u64, width: usize) -> String {
    let filled = if total == 0 {
        0
    } else {
        ((current.min(total) as u128 * width as u128) / total as u128) as usize
    };
    format!("[{}{}]", "█".repeat(filled), "░".repeat(width - filled))
}

fn colored_progress_bar(current: u64, total: u64, width: usize, no_color: bool) -> String {
    use std::fmt::Write as _;
    if no_color {
        return progress_bar(current, total, width);
    }
    let filled = if total == 0 {
        0
    } else {
        ((current.min(total) as u128 * width as u128) / total as u128) as usize
    };
    let mut output = String::from("[");
    for position in 0..filled {
        let (red, green, blue) = gradient_color(position, width.saturating_sub(1));
        let _ = write!(output, "\x1b[38;2;{red};{green};{blue}m█");
    }
    if filled < width {
        output.push_str("\x1b[38;2;48;48;55m");
        output.push_str(&"░".repeat(width - filled));
    }
    output.push_str(ANSI_RESET);
    output.push(']');
    output
}

fn colorize_patch_row(row: &str, no_color: bool) -> String {
    if no_color {
        return row.to_string();
    }
    let mut output = String::new();
    let mut inside_map = false;
    for character in row.chars() {
        match character {
            '[' => {
                inside_map = true;
                output.push('[');
            }
            ']' => {
                inside_map = false;
                output.push_str(ANSI_RESET);
                output.push(']');
            }
            'R' if inside_map => {
                let _ = std::fmt::Write::write_fmt(
                    &mut output,
                    format_args!(
                        "\x1b[38;2;{};{};{}mR",
                        BRAND_VIOLET.0, BRAND_VIOLET.1, BRAND_VIOLET.2
                    ),
                );
            }
            'D' if inside_map => {
                let _ = std::fmt::Write::write_fmt(
                    &mut output,
                    format_args!(
                        "\x1b[38;2;{};{};{}mD",
                        BRAND_MAGENTA.0, BRAND_MAGENTA.1, BRAND_MAGENTA.2
                    ),
                );
            }
            _ => output.push(character),
        }
    }
    output.push_str(ANSI_RESET);
    output
}

fn sanitize_key(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
        .take(48)
        .collect()
}

fn sanitize_text(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(512)
        .collect()
}

fn sanitize_field(key: &str, value: &str) -> String {
    let normalized = key.to_ascii_lowercase();
    if ["url", "authorization", "cookie", "token", "secret", "path"]
        .iter()
        .any(|sensitive| normalized.contains(sensitive))
    {
        return "[REDACTED]".into();
    }
    sanitize_text(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_runtime_is_a_no_op() {
        let runtime = DebugRuntime::default();
        assert!(!runtime.is_enabled());
        runtime.stage(DebugScope::System, "test", "ignored");
    }

    #[test]
    fn emits_versioned_sanitized_events() {
        let (sender, receiver) = mpsc::sync_channel(2);
        let runtime = DebugRuntime::for_sender("session", sender);
        runtime.emit(
            DebugScope::Api,
            DebugEventKind::ApiCall,
            Some("request\nstart"),
            Some("Calling API"),
            None,
            [
                ("signed_url".into(), "https://secret.example".into()),
                ("method".into(), "POST".into()),
            ],
        );
        let event = receiver.recv().unwrap();
        assert_eq!(event.schema_version, 1);
        assert_eq!(event.sequence, 1);
        assert_eq!(event.phase.as_deref(), Some("request start"));
        assert_eq!(event.fields.get("signed_url").unwrap(), "[REDACTED]");
        assert_eq!(event.fields.get("method").unwrap(), "POST");
    }

    #[test]
    fn queue_pressure_never_blocks_the_caller() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let runtime = DebugRuntime::for_sender("session", sender);
        runtime.stage(DebugScope::System, "one", "first");
        runtime.stage(DebugScope::System, "two", "dropped");
    }

    #[test]
    fn progress_bar_clamps_and_handles_unknown_totals() {
        assert_eq!(progress_bar(5, 10, 10), "[█████░░░░░]");
        assert_eq!(progress_bar(20, 10, 4), "[████]");
        assert_eq!(progress_bar(1, 0, 3), "[░░░]");
    }

    #[test]
    fn logo_spells_the_product_name() {
        let header = brand_header("dbg-test", true);
        assert_eq!(header.lines().count(), 9);
        assert!(header.contains("INCREMENTAL DISTRIBUTION DEBUG CONSOLE"));
        assert!(header.contains("dbg-test"));
        assert!(!header.contains("\x1b["));
        assert!(
            header
                .lines()
                .all(|line| line.chars().count() <= CONSOLE_WIDTH),
            "the brand must fit the default companion terminal"
        );
    }

    #[test]
    fn colored_branding_uses_truecolor_and_patch_map_colors() {
        let header = brand_header("dbg-test", false);
        assert!(header.contains("\x1b[38;2;"));
        assert!(header.contains("252;250;255m"));
        assert!(header.contains("224;39;246m"));
        assert_eq!(vertical_brand_color(0, 5), BRAND_TOP);
        assert_eq!(vertical_brand_color(5, 5), BRAND_BOTTOM);

        let row = colorize_patch_row("game.pak [RRDD] reuse 50%", false);
        assert!(row.contains("105;61;255mR"));
        assert!(row.contains("255;43;178mD"));
        assert_eq!(
            colorize_patch_row("game.pak [RRDD]", true),
            "game.pak [RRDD]"
        );
    }
}
