use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_ARCHIVE_FILES: usize = 100_000;

#[derive(Debug)]
struct Options {
    archive: PathBuf,
    entrypoint: String,
    working_directory: Option<String>,
    launch_arguments: Vec<String>,
    executables: Vec<String>,
    environment: BTreeMap<String, String>,
    output: Option<PathBuf>,
}

#[derive(Debug, Serialize)]
struct UploadDeclaration {
    platform: &'static str,
    architecture: &'static str,
    archive_format: &'static str,
    compressed_size_bytes: String,
    installed_size_bytes: String,
    sha256: String,
    manifest: ManifestDeclaration,
}

#[derive(Debug, Serialize)]
struct ManifestDeclaration {
    schema_version: &'static str,
    entrypoint: String,
    launch_arguments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    working_directory: Option<String>,
    executables: Vec<String>,
    environment: BTreeMap<String, String>,
}

#[derive(Debug)]
struct ArchiveInspection {
    installed_size_bytes: u64,
    files: HashSet<String>,
    directories: HashSet<String>,
}

fn main() {
    match run(env::args().skip(1)) {
        Ok(()) => {}
        Err(error) => {
            eprintln!("artifact preflight failed: {error}");
            std::process::exit(1);
        }
    }
}

fn run(arguments: impl Iterator<Item = String>) -> Result<(), String> {
    let options = parse_options(arguments)?;
    if options.output.as_ref() == Some(&options.archive) {
        return Err("output path must not overwrite the ZIP archive".into());
    }
    let output = options.output.clone();
    let declaration = preflight(options)?;
    let json = serde_json::to_string_pretty(&declaration)
        .map_err(|error| format!("could not serialize upload declaration: {error}"))?;
    if let Some(path) = output {
        fs::write(&path, format!("{json}\n"))
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        eprintln!("artifact preflight passed: {}", path.display());
    } else {
        println!("{json}");
    }
    Ok(())
}

fn parse_options(arguments: impl Iterator<Item = String>) -> Result<Options, String> {
    let mut arguments = arguments.peekable();
    let archive = arguments.next().map(PathBuf::from).ok_or_else(usage)?;
    let mut entrypoint = None;
    let mut working_directory = None;
    let mut launch_arguments = Vec::new();
    let mut executables = Vec::new();
    let mut environment = BTreeMap::new();
    let mut output = None;

    while let Some(flag) = arguments.next() {
        let value = arguments
            .next()
            .ok_or_else(|| format!("{flag} requires a value\n{}", usage()))?;
        match flag.as_str() {
            "--entrypoint" => entrypoint = Some(value),
            "--working-directory" => working_directory = Some(value),
            "--launch-argument" => launch_arguments.push(value),
            "--executable" => executables.push(value),
            "--environment" => {
                let (key, value) = value
                    .split_once('=')
                    .filter(|(key, _)| !key.is_empty())
                    .ok_or("--environment must use KEY=VALUE")?;
                environment.insert(key.to_string(), value.to_string());
            }
            "--output" => output = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option {flag}\n{}", usage())),
        }
    }

    let entrypoint = entrypoint.ok_or_else(usage)?;
    Ok(Options {
        archive,
        entrypoint,
        working_directory,
        launch_arguments,
        executables,
        environment,
        output,
    })
}

fn usage() -> String {
    "usage: artifact-preflight <archive.zip> --entrypoint <relative-path> [--working-directory <relative-path>] [--launch-argument <value>] [--executable <relative-path>] [--environment KEY=VALUE] [--output <declaration.json>]".into()
}

fn preflight(options: Options) -> Result<UploadDeclaration, String> {
    if !options.archive.is_file() {
        return Err(format!(
            "archive does not exist: {}",
            options.archive.display()
        ));
    }
    if options
        .archive
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Err("the Windows MVP artifact must use the .zip extension".into());
    }

    let entrypoint = normalize_relative_path(&options.entrypoint)?;
    let working_directory = options
        .working_directory
        .as_deref()
        .map(normalize_relative_path)
        .transpose()?;
    let mut executables = options
        .executables
        .iter()
        .map(|path| normalize_relative_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    let mut seen_executables = HashSet::new();
    executables.retain(|path| seen_executables.insert(path.to_ascii_lowercase()));
    if !executables
        .iter()
        .any(|path| path.eq_ignore_ascii_case(&entrypoint))
    {
        executables.insert(0, entrypoint.clone());
    }
    let inspection = inspect_zip(&options.archive)?;

    if !inspection.files.contains(&entrypoint) {
        return Err(format!(
            "archive does not contain the declared entrypoint: {entrypoint}"
        ));
    }
    for executable in &executables {
        if !inspection.files.contains(executable) {
            return Err(format!(
                "archive does not contain declared executable: {executable}"
            ));
        }
    }
    if let Some(directory) = &working_directory {
        let prefix = format!("{directory}/");
        if !inspection.directories.contains(directory)
            && !inspection
                .files
                .iter()
                .any(|path| path.starts_with(&prefix))
        {
            return Err(format!(
                "archive does not contain the working directory: {directory}"
            ));
        }
    }

    let compressed_size = options
        .archive
        .metadata()
        .map_err(|error| format!("could not read archive metadata: {error}"))?
        .len();
    if compressed_size == 0 || inspection.installed_size_bytes == 0 {
        return Err("artifact sizes must be greater than zero".into());
    }

    Ok(UploadDeclaration {
        platform: "WINDOWS",
        architecture: "X86_64",
        archive_format: "ZIP",
        compressed_size_bytes: compressed_size.to_string(),
        installed_size_bytes: inspection.installed_size_bytes.to_string(),
        sha256: sha256_file(&options.archive)?,
        manifest: ManifestDeclaration {
            schema_version: "1",
            entrypoint,
            launch_arguments: options.launch_arguments,
            working_directory,
            executables,
            environment: options.environment,
        },
    })
}

fn normalize_relative_path(value: &str) -> Result<String, String> {
    if value.is_empty() || value.starts_with('/') || value.starts_with('\\') || value.contains(':')
    {
        return Err(format!("unsafe artifact path: {value}"));
    }
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe artifact path: {value}"));
    }
    Ok(normalized)
}

fn inspect_zip(path: &Path) -> Result<ArchiveInspection, String> {
    let file = File::open(path).map_err(|error| format!("could not open archive: {error}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| format!("invalid ZIP archive: {error}"))?;
    if archive.is_empty() {
        return Err("archive is empty".into());
    }
    if archive.len() > MAX_ARCHIVE_FILES {
        return Err(format!(
            "archive contains more than {MAX_ARCHIVE_FILES} entries"
        ));
    }

    let mut installed_size_bytes = 0_u64;
    let mut paths = HashSet::new();
    let mut files = HashSet::new();
    let mut directories = HashSet::new();
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read ZIP entry {index}: {error}"))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(format!("symbolic links are not allowed: {}", entry.name()));
        }
        entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe ZIP entry: {}", entry.name()))?;
        let normalized = normalize_relative_path(entry.name().trim_end_matches('/'))?;
        if !paths.insert(normalized.to_ascii_lowercase()) {
            return Err(format!("duplicate ZIP entry: {normalized}"));
        }

        if entry.is_dir() {
            directories.insert(normalized);
            continue;
        }
        installed_size_bytes = installed_size_bytes
            .checked_add(entry.size())
            .ok_or("installed size overflow")?;
        std::io::copy(&mut entry, &mut std::io::sink())
            .map_err(|error| format!("could not verify ZIP entry {normalized}: {error}"))?;
        files.insert(normalized);
    }

    Ok(ArchiveInspection {
        installed_size_bytes,
        files,
        directories,
    })
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| format!("could not hash archive: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("could not hash archive: {error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
    }

    fn options(path: PathBuf, entrypoint: &str) -> Options {
        Options {
            archive: path,
            entrypoint: entrypoint.into(),
            working_directory: None,
            launch_arguments: Vec::new(),
            executables: vec![entrypoint.into()],
            environment: BTreeMap::new(),
            output: None,
        }
    }

    #[test]
    fn emits_the_exact_publisher_upload_declaration() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(&path, &[("bin/game.exe", b"game"), ("data.bin", b"data")]);

        let declaration = preflight(options(path.clone(), "bin\\game.exe")).unwrap();
        let json = serde_json::to_value(&declaration).unwrap();

        assert_eq!(json["platform"], "WINDOWS");
        assert_eq!(json["architecture"], "X86_64");
        assert_eq!(json["archive_format"], "ZIP");
        assert_eq!(json["installed_size_bytes"], "8");
        assert_eq!(
            json["compressed_size_bytes"],
            path.metadata().unwrap().len().to_string()
        );
        assert_eq!(json["manifest"]["schema_version"], "1");
        assert_eq!(json["manifest"]["entrypoint"], "bin/game.exe");
        assert_eq!(json["sha256"].as_str().unwrap().len(), 64);
    }

    #[test]
    fn writes_a_utf8_json_declaration_file() {
        let directory = tempfile::tempdir().unwrap();
        let archive = directory.path().join("game.zip");
        let output = directory.path().join("upload-declaration.json");
        write_zip(&archive, &[("game.exe", b"game")]);

        run(vec![
            archive.to_string_lossy().into_owned(),
            "--entrypoint".into(),
            "game.exe".into(),
            "--output".into(),
            output.to_string_lossy().into_owned(),
        ]
        .into_iter())
        .unwrap();

        let bytes = fs::read(output).unwrap();
        assert!(std::str::from_utf8(&bytes).is_ok());
        let declaration: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(declaration["manifest"]["entrypoint"], "game.exe");
    }

    #[test]
    fn rejects_unsafe_or_missing_entrypoints() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(&path, &[("game.exe", b"game")]);

        assert!(preflight(options(path.clone(), "../game.exe")).is_err());
        assert!(preflight(options(path, "missing.exe"))
            .unwrap_err()
            .contains("declared entrypoint"));
    }

    #[test]
    fn rejects_archive_path_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(&path, &[("../escape.exe", b"bad")]);

        assert!(preflight(options(path, "escape.exe"))
            .unwrap_err()
            .contains("unsafe ZIP entry"));
    }
}
