use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Manager, Runtime};

use crate::debug::{self, DebugEventKind, DebugRuntime, DebugScope};

pub const BUTLER_VERSION: &str = "15.30.0";
pub const WHARF_ALGORITHM: &str = "WHARF";
pub const WHARF_FORMAT_VERSION: &str = "1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButlerFilePin {
    pub name: &'static str,
    pub sha256: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButlerTargetPin {
    pub target: &'static str,
    pub executable: &'static str,
    pub files: &'static [ButlerFilePin],
}

const WINDOWS_AMD64: &[ButlerFilePin] = &[
    ButlerFilePin {
        name: "butler.exe",
        sha256: "1099ebacba44c5e781babdc0cc409ba91010e284e9ca000e61753e8aa0e84be2",
    },
    ButlerFilePin {
        name: "7z.dll",
        sha256: "4b77ce85d5cac538cc2b1a2d498af607dd5650975fd67549422702545290ef57",
    },
    ButlerFilePin {
        name: "c7zip.dll",
        sha256: "9f79ae9b22d4b5a608eb35987419731ca85c7e531c9a9fa0af8099f783b6b9f5",
    },
];
const LINUX_AMD64: &[ButlerFilePin] = &[
    ButlerFilePin {
        name: "butler",
        sha256: "f32d1d932528c3a0c4c0471d721dfe0c7c24fb16a0fc4e3e81f5a118e0b6d790",
    },
    ButlerFilePin {
        name: "7z.so",
        sha256: "334ed1aaaacd3ddefb41db6ae7c3d766d40782095e7a1ed6c7105b3ca9d1ba88",
    },
    ButlerFilePin {
        name: "libc7zip.so",
        sha256: "0370a19507b3c54e3ee8730feb344f54dc5819775f2c80a27d970084f9178c7c",
    },
];
const LINUX_ARM64: &[ButlerFilePin] = &[
    ButlerFilePin {
        name: "butler",
        sha256: "474a1c47c133d6d8e01b47f033c23abaf1e30987a6928eea812d78e1e30b913d",
    },
    ButlerFilePin {
        name: "7z.so",
        sha256: "a323bce742109c23c89a4353e44f47ce36bb7b71c4ac8574e00ddedacd587868",
    },
    ButlerFilePin {
        name: "libc7zip.so",
        sha256: "3c745898e13fa7084a1c0e51ec11864ba612ba5f4dc883a874c76c814d2c63d9",
    },
];
const DARWIN_AMD64: &[ButlerFilePin] = &[
    ButlerFilePin {
        name: "butler",
        sha256: "30f3c79fff5efe34474316402c23cccd9164167b25c05c743be5b130c62cd304",
    },
    ButlerFilePin {
        name: "7z.so",
        sha256: "ce3c2af11eead60ffb9d37aff77ab434fc48663b3c2a1d40586259a1bac4ac6d",
    },
    ButlerFilePin {
        name: "libc7zip.dylib",
        sha256: "e258106d10224324c3e13be3d56cf027c5d3fafc84d97edf3d51452b9b79730b",
    },
];
const DARWIN_ARM64: &[ButlerFilePin] = &[
    ButlerFilePin {
        name: "butler",
        sha256: "aa5a9591a81ee968f89f45526d7a961fa96c7370f6e18559046a33dfcc81af96",
    },
    ButlerFilePin {
        name: "7z.so",
        sha256: "cd03e851931f1e5e356e44ad3e611e9cdb7c7985199a214bc915fffe8e405dcf",
    },
    ButlerFilePin {
        name: "libc7zip.dylib",
        sha256: "7715bc55baf65ea14c818f0fb816f5dc997292af6a4fd0a7b4a3be8095259909",
    },
];

pub const ALL_TARGETS: &[ButlerTargetPin] = &[
    ButlerTargetPin {
        target: "windows-x86_64",
        executable: "butler.exe",
        files: WINDOWS_AMD64,
    },
    ButlerTargetPin {
        target: "linux-x86_64",
        executable: "butler",
        files: LINUX_AMD64,
    },
    ButlerTargetPin {
        target: "linux-aarch64",
        executable: "butler",
        files: LINUX_ARM64,
    },
    ButlerTargetPin {
        target: "macos-x86_64",
        executable: "butler",
        files: DARWIN_AMD64,
    },
    ButlerTargetPin {
        target: "macos-aarch64",
        executable: "butler",
        files: DARWIN_ARM64,
    },
];

fn current_pin() -> Result<&'static ButlerTargetPin, String> {
    let target = if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "windows-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "linux-x86_64"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "linux-aarch64"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "macos-x86_64"
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "macos-aarch64"
    } else {
        return Err("Butler is not pinned for this platform and architecture".into());
    };
    ALL_TARGETS
        .iter()
        .find(|pin| pin.target == target)
        .ok_or_else(|| "missing Butler target pin".into())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("could not open pinned Butler file: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("could not read pinned Butler file: {error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

#[derive(Clone, Debug)]
pub struct Butler {
    executable: PathBuf,
    debug: DebugRuntime,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct ButlerDiagnostics {
    progress: Option<u8>,
    event_count: usize,
    stderr_bytes: usize,
}

fn read_structured_output(
    reader: impl Read,
    structured: bool,
    debug: Option<(DebugRuntime, String)>,
) -> ButlerDiagnostics {
    let mut diagnostics = ButlerDiagnostics::default();
    for line in BufReader::new(reader).lines().map_while(Result::ok) {
        // Keep draining the pipe after the diagnostic cap so Butler can never
        // fail with a broken pipe while producing a large diff or apply log.
        if diagnostics.event_count >= 512 {
            continue;
        }
        if line.len() > 4096 {
            continue;
        }
        if !structured {
            diagnostics.event_count += 1;
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        let kind = value.get("type").and_then(serde_json::Value::as_str);
        if !matches!(kind, Some("log" | "progress")) {
            continue;
        }
        diagnostics.event_count += 1;
        if kind == Some("progress") {
            diagnostics.progress = value
                .get("percentage")
                .and_then(|value| {
                    value
                        .as_str()
                        .and_then(|value| value.parse::<u8>().ok())
                        .or_else(|| value.as_u64().and_then(|value| u8::try_from(value).ok()))
                })
                .filter(|value| *value <= 100);
            if let (Some(percentage), Some((runtime, operation))) =
                (diagnostics.progress, debug.as_ref())
            {
                runtime.progress(
                    DebugScope::Butler,
                    operation,
                    u64::from(percentage),
                    100,
                    "percent",
                );
            }
        } else if let (Some(message), Some((runtime, operation))) = (
            value.get("message").and_then(serde_json::Value::as_str),
            debug.as_ref(),
        ) {
            if let Some(message) = safe_butler_message(message) {
                runtime.emit(
                    DebugScope::Butler,
                    DebugEventKind::Message,
                    Some(operation),
                    Some(&message),
                    None,
                    [],
                );
            }
        }
    }
    diagnostics
}

fn safe_butler_message(message: &str) -> Option<String> {
    let message = message.replace(['\r', '\n'], " ");
    let lower = message.to_ascii_lowercase();
    if message.contains(['\\', '/'])
        || lower.contains("://")
        || lower.contains("token")
        || lower.contains("authorization")
    {
        return None;
    }
    Some(message.chars().take(512).collect())
}

fn read_controlled_stderr(reader: impl Read) -> usize {
    let mut reader = reader;
    let mut buffer = [0_u8; 8192];
    let mut retained_bytes = 0_usize;
    while let Ok(read) = reader.read(&mut buffer) {
        if read == 0 {
            break;
        }
        retained_bytes = retained_bytes.saturating_add(read).min(16 * 1024);
    }
    retained_bytes
}
impl Butler {
    pub fn locate<R: Runtime>(app: &AppHandle<R>) -> Result<Self, String> {
        let pin = current_pin()?;
        let root = app
            .path()
            .resource_dir()
            .map_err(|error| format!("could not resolve app resources: {error}"))?
            .join("butler")
            .join(BUTLER_VERSION)
            .join(pin.target);
        for file in pin.files {
            let path = root.join(file.name);
            let actual = sha256_file(&path)?;
            if actual != file.sha256 {
                return Err(format!(
                    "pinned Butler file failed SHA-256 verification: {}",
                    file.name
                ));
            }
        }
        Ok(Self {
            executable: root.join(pin.executable),
            debug: debug::runtime(app),
        })
    }

    #[cfg(test)]
    fn from_executable(executable: PathBuf) -> Self {
        Self {
            executable,
            debug: DebugRuntime::default(),
        }
    }

    fn diff_arguments(old: &Path, new: &Path, patch: &Path) -> Vec<String> {
        vec![
            "-j".into(),
            "diff".into(),
            old.to_string_lossy().into_owned(),
            new.to_string_lossy().into_owned(),
            patch.to_string_lossy().into_owned(),
            "--verify".into(),
        ]
    }

    fn apply_arguments(
        patch: &Path,
        signature: &Path,
        source: &Path,
        destination: Option<&Path>,
        staging: &Path,
    ) -> Vec<String> {
        let mut arguments = vec![
            "-j".into(),
            "apply".into(),
            "--staging-dir".into(),
            staging.to_string_lossy().into_owned(),
        ];
        if let Some(destination) = destination {
            arguments.push("--dir".into());
            arguments.push(destination.to_string_lossy().into_owned());
        }
        arguments.extend([
            "--signature".into(),
            signature.to_string_lossy().into_owned(),
            patch.to_string_lossy().into_owned(),
            source.to_string_lossy().into_owned(),
        ]);
        arguments
    }

    fn verify_arguments(signature: &Path, target: &Path) -> Vec<String> {
        vec![
            "-j".into(),
            "verify".into(),
            signature.to_string_lossy().into_owned(),
            target.to_string_lossy().into_owned(),
        ]
    }

    pub fn diff(
        &self,
        old: &Path,
        new: &Path,
        patch: &Path,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        self.run(Self::diff_arguments(old, new, patch), cancellation)
            .map(|_| ())
    }

    pub fn apply_to(
        &self,
        patch: &Path,
        signature: &Path,
        source: &Path,
        destination: &Path,
        staging: &Path,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        self.run(
            Self::apply_arguments(patch, signature, source, Some(destination), staging),
            cancellation,
        )
        .map(|_| ())
    }

    pub fn apply(
        &self,
        patch: &Path,
        signature: &Path,
        target: &Path,
        staging: &Path,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        self.run(
            Self::apply_arguments(patch, signature, target, None, staging),
            cancellation,
        )
        .map(|_| ())
    }

    pub fn verify(
        &self,
        signature: &Path,
        target: &Path,
        cancellation: &AtomicBool,
    ) -> Result<(), String> {
        self.run(Self::verify_arguments(signature, target), cancellation)
            .map(|_| ())
    }

    fn run(
        &self,
        args: Vec<String>,
        cancellation: &AtomicBool,
    ) -> Result<ButlerDiagnostics, String> {
        self.run_internal(args, cancellation, true)
    }

    fn run_internal(
        &self,
        args: Vec<String>,
        cancellation: &AtomicBool,
        structured: bool,
    ) -> Result<ButlerDiagnostics, String> {
        let operation = args
            .iter()
            .find(|argument| matches!(argument.as_str(), "diff" | "apply" | "verify"))
            .map(String::as_str)
            .unwrap_or("process")
            .to_string();
        self.debug.emit(
            DebugScope::Butler,
            DebugEventKind::Stage,
            Some(&operation),
            Some("Starting the pinned offline Butler sidecar."),
            None,
            [("butler_version".into(), BUTLER_VERSION.into())],
        );
        if cancellation.load(Ordering::Relaxed) {
            return Err("installation cancelled".into());
        }
        let mut command = Command::new(&self.executable);
        command
            .args(&args)
            .current_dir(self.executable.parent().ok_or("invalid Butler path")?)
            .env_clear()
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Ok(value) = std::env::var("TEMP") {
            command.env("TEMP", value);
        }
        if let Ok(value) = std::env::var("TMP") {
            command.env("TMP", value);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("could not start pinned Butler: {error}"))?;
        let stdout = child.stdout.take().ok_or("could not read Butler output")?;
        let stderr = child.stderr.take().ok_or("could not read Butler errors")?;
        let debug = self.debug.clone();
        let reader_operation = operation.clone();
        let output_reader = thread::spawn(move || {
            read_structured_output(stdout, structured, Some((debug, reader_operation)))
        });
        let error_reader = thread::spawn(move || read_controlled_stderr(stderr));
        loop {
            if cancellation.load(Ordering::Relaxed) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = output_reader.join();
                let _ = error_reader.join();
                self.debug.emit(
                    DebugScope::Butler,
                    DebugEventKind::Warning,
                    Some(&operation),
                    Some("Butler was cancelled; the active installation was not changed."),
                    None,
                    [],
                );
                return Err("installation cancelled".into());
            }
            if let Some(status) = child
                .try_wait()
                .map_err(|error| format!("could not monitor Butler: {error}"))?
            {
                let mut diagnostics = output_reader.join().unwrap_or_default();
                diagnostics.stderr_bytes = error_reader.join().unwrap_or(0);
                return if status.success() {
                    self.debug.emit(
                        DebugScope::Butler,
                        DebugEventKind::Complete,
                        Some(&operation),
                        Some("Butler completed successfully."),
                        Some((100, 100, "percent")),
                        [("events".into(), diagnostics.event_count.to_string())],
                    );
                    Ok(diagnostics)
                } else {
                    self.debug.emit(
                        DebugScope::Butler,
                        DebugEventKind::Error,
                        Some(&operation),
                        Some("Butler exited unsuccessfully; no sensitive stderr was forwarded."),
                        None,
                        [
                            ("events".into(), diagnostics.event_count.to_string()),
                            ("stderr_bytes".into(), diagnostics.stderr_bytes.to_string()),
                        ],
                    );
                    Err(format!(
                        "Butler exited unsuccessfully (progress: {:?}, events: {}, stderr bytes: {})",
                        diagnostics.progress, diagnostics.event_count, diagnostics.stderr_bytes
                    ))
                };
            }
            thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn every_supported_target_has_unique_lowercase_sha256_pins() {
        assert_eq!(ALL_TARGETS.len(), 5);
        for target in ALL_TARGETS {
            assert!(target
                .files
                .iter()
                .any(|file| file.name == target.executable));
            for file in target.files {
                assert_eq!(file.sha256.len(), 64);
                assert!(file
                    .sha256
                    .chars()
                    .all(|value| value.is_ascii_hexdigit() && !value.is_ascii_uppercase()));
            }
        }
    }

    #[test]
    fn command_arguments_keep_official_order_and_json_mode() {
        let old = Path::new("old");
        let new = Path::new("new");
        let patch = Path::new("patch.pwr");
        let signature = Path::new("patch.pwr.sig");
        let staging = Path::new("staging");
        assert_eq!(
            Butler::diff_arguments(old, new, patch),
            vec!["-j", "diff", "old", "new", "patch.pwr", "--verify"]
        );
        assert_eq!(
            Butler::apply_arguments(patch, signature, old, None, staging),
            vec![
                "-j",
                "apply",
                "--staging-dir",
                "staging",
                "--signature",
                "patch.pwr.sig",
                "patch.pwr",
                "old"
            ]
        );
        assert_eq!(
            Butler::apply_arguments(patch, signature, old, Some(new), staging),
            vec![
                "-j",
                "apply",
                "--staging-dir",
                "staging",
                "--dir",
                "new",
                "--signature",
                "patch.pwr.sig",
                "patch.pwr",
                "old"
            ]
        );
        assert_eq!(
            Butler::verify_arguments(signature, new),
            vec!["-j", "verify", "patch.pwr.sig", "new"]
        );
    }

    #[test]
    fn json_output_retains_only_bounded_diagnostics() {
        let input = br#"{"type":"log","message":"secret path C:\\private"}
{"type":"progress","percentage":"80"}
not-json
"#;
        let diagnostics = read_structured_output(&input[..], true, None);
        assert_eq!(diagnostics.progress, Some(80));
        assert_eq!(diagnostics.event_count, 2);
    }

    #[test]
    fn debug_forwarding_rejects_butler_paths_urls_and_tokens() {
        assert_eq!(
            safe_butler_message("Re-used 92.4% of old, added 38.2 MiB fresh data").as_deref(),
            Some("Re-used 92.4% of old, added 38.2 MiB fresh data")
        );
        assert!(safe_butler_message(r"Reading C:\private\game.zip").is_none());
        assert!(safe_butler_message("GET https://storage.test/signed").is_none());
        assert!(safe_butler_message("authorization token refreshed").is_none());
    }

    #[test]
    fn diagnostics_are_capped_without_stopping_pipe_drainage() {
        use std::io::Cursor;

        let input = (0..600).map(|_| "{\"type\":\"log\"}\n").collect::<String>();
        let mut reader = Cursor::new(input.as_bytes());
        let diagnostics = read_structured_output(&mut reader, true, None);
        assert_eq!(diagnostics.event_count, 512);
        assert_eq!(reader.position(), input.len() as u64);

        let stderr = vec![b'x'; 32 * 1024];
        let mut stderr_reader = Cursor::new(stderr.as_slice());
        assert_eq!(read_controlled_stderr(&mut stderr_reader), 16 * 1024);
        assert_eq!(stderr_reader.position(), stderr.len() as u64);
    }

    #[cfg(windows)]
    #[test]
    fn cancellation_terminates_the_real_child_process() {
        use std::sync::Arc;
        let cancellation = Arc::new(AtomicBool::new(false));
        let signal = cancellation.clone();
        let runner = Butler::from_executable(PathBuf::from("C:\\Windows\\System32\\cmd.exe"));
        let thread = std::thread::spawn(move || {
            runner.run_internal(
                vec!["/C".into(), "ping 127.0.0.1 -n 30 >NUL".into()],
                &signal,
                false,
            )
        });
        std::thread::sleep(Duration::from_millis(100));
        cancellation.store(true, Ordering::Relaxed);
        assert_eq!(
            thread.join().unwrap().unwrap_err(),
            "installation cancelled"
        );
    }
    #[test]
    #[ignore = "requires npm run sidecar:prepare for the current target"]
    fn prepared_sidecar_real_diff_apply_verify() {
        let pin = current_pin().unwrap();
        let executable = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("butler")
            .join(BUTLER_VERSION)
            .join(pin.target)
            .join(pin.executable);
        assert!(
            executable.is_file(),
            "prepare the pinned sidecar first with npm run sidecar:prepare"
        );
        for file in pin.files {
            assert_eq!(
                sha256_file(&executable.parent().unwrap().join(file.name)).unwrap(),
                file.sha256
            );
        }

        let directory = tempfile::tempdir().unwrap();
        let old = directory.path().join("old");
        let new = directory.path().join("new");
        let rebuilt = directory.path().join("rebuilt");
        let staging = directory.path().join("staging");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        fs::create_dir_all(&staging).unwrap();
        fs::write(old.join("game.txt"), b"version one").unwrap();
        fs::write(new.join("game.txt"), b"version two").unwrap();
        fs::write(new.join("new-file.txt"), b"added").unwrap();
        let old_asset = vec![b'a'; 4 * 64 * 1024];
        let mut new_asset = old_asset.clone();
        new_asset[64 * 1024..2 * 64 * 1024].fill(b'b');
        fs::write(old.join("asset.bin"), old_asset).unwrap();
        fs::write(new.join("asset.bin"), new_asset).unwrap();
        let patch = directory.path().join("update.pwr");
        let signature = directory.path().join("update.pwr.sig");
        let cancellation = AtomicBool::new(false);
        let butler = Butler::from_executable(executable);

        butler.diff(&old, &new, &patch, &cancellation).unwrap();
        assert!(patch.is_file());
        assert!(signature.is_file());
        let analysis = crate::pwr_inspector::inspect_patch(&patch).unwrap();
        assert_eq!(analysis.old_file_count, 2);
        assert_eq!(analysis.new_file_count, 3);
        assert!(analysis.reused_bytes > 0);
        assert!(analysis.fresh_bytes > 0);
        assert!(analysis.operation_count > 0);
        let asset = analysis
            .files
            .iter()
            .find(|file| file.path == "asset.bin")
            .unwrap();
        assert!(asset.block_map(8).contains('R'));
        assert!(asset.block_map(8).contains('D'));
        butler
            .apply_to(&patch, &signature, &old, &rebuilt, &staging, &cancellation)
            .unwrap();
        butler.verify(&signature, &rebuilt, &cancellation).unwrap();
        assert_eq!(fs::read(rebuilt.join("game.txt")).unwrap(), b"version two");
        assert_eq!(fs::read(rebuilt.join("new-file.txt")).unwrap(), b"added");
    }
}
