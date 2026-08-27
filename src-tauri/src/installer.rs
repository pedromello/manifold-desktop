use futures_util::StreamExt;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

const REGISTRY_FILE: &str = "installations.json";
const PREFERENCES_FILE: &str = "installation-preferences.json";
const INSTALLATION_LOG_FILE: &str = "installation.log";
const MAX_INSTALLATION_LOG_BYTES: u64 = 1_000_000;
const MAX_DIAGNOSTIC_EVENTS: usize = 100;
const MAX_ARCHIVE_FILES: usize = 100_000;
const DOWNLOAD_AUTHORIZATION_EXPIRED: &str = "download authorization expired";
const DOWNLOAD_INTERRUPTED: &str = "download interrupted after automatic retries";
const MAX_TRANSIENT_DOWNLOAD_RETRIES: u32 = 4;
const DOWNLOAD_RETRY_BASE_DELAY_MS: u64 = 200;
const DOWNLOAD_RETRY_MAX_DELAY_MS: u64 = 3_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallCommandError {
    code: String,
    message: String,
    retryable: bool,
}

impl InstallCommandError {
    fn from_message(message: String) -> Self {
        Self {
            code: match message.as_str() {
                DOWNLOAD_AUTHORIZATION_EXPIRED => "DOWNLOAD_AUTHORIZATION_EXPIRED",
                DOWNLOAD_INTERRUPTED => "DOWNLOAD_INTERRUPTED",
                _ => "INSTALLATION_FAILED",
            }
            .into(),
            message,
            retryable: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseTarget {
    pub platform: String,
    pub architecture: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleaseSummary {
    pub id: String,
    pub version: String,
    pub release_number: u64,
    pub published_at: String,
    pub artifact_id: String,
    pub target: ReleaseTarget,
    pub compressed_size_bytes: String,
    pub installed_size_bytes: String,
    pub sha256: String,
    pub manifest_schema_version: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct InstallManifest {
    pub schema_version: String,
    pub release_id: String,
    pub artifact_id: String,
    pub entrypoint: String,
    #[serde(default)]
    pub launch_arguments: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub executables: Vec<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DownloadAuthorization {
    pub artifact_id: String,
    pub url: String,
    pub expires_at: String,
    pub total_size_bytes: String,
    pub sha256: String,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DistributionPlan {
    pub game_slug: String,
    pub release: ReleaseSummary,
    pub manifest: InstallManifest,
    pub download: DownloadAuthorization,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledGame {
    pub game_slug: String,
    pub title: String,
    pub version: String,
    pub release_id: String,
    #[serde(default)]
    pub release_number: u64,
    #[serde(default)]
    pub artifact_id: String,
    #[serde(default)]
    pub installed_size_bytes: String,
    pub install_directory: String,
    pub entrypoint: String,
    pub installed_at: String,
    #[serde(default)]
    pub status: InstallationStatus,
    #[serde(default)]
    pub launch_arguments: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InstallationStatus {
    #[default]
    Installed,
    RepairNeeded,
}

#[derive(Debug)]
struct LaunchPlan {
    executable: PathBuf,
    working_directory: PathBuf,
    arguments: Vec<String>,
    environment: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
struct StoredPreferences {
    install_directory: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationPreferences {
    install_directory: Option<String>,
    default_install_directory: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationDiagnosticEvent {
    timestamp: u64,
    game_slug: String,
    event: String,
    release_id: Option<String>,
    artifact_id: Option<String>,
    version: Option<String>,
    total_bytes: Option<String>,
    error_code: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationDiagnostics {
    app_version: &'static str,
    events: Vec<InstallationDiagnosticEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallationProgress {
    game_slug: String,
    title: String,
    phase: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    version: Option<String>,
    error: Option<String>,
}

pub struct InstallationManager {
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl InstallationManager {
    pub fn new() -> Self {
        Self {
            cancellations: Mutex::new(HashMap::new()),
        }
    }

    fn begin(&self, game_slug: &str) -> Result<Arc<AtomicBool>, String> {
        let mut jobs = self
            .cancellations
            .lock()
            .map_err(|_| "the installation manager is unavailable".to_string())?;
        if jobs.contains_key(game_slug) {
            return Err("an installation is already running for this game".into());
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        jobs.insert(game_slug.to_string(), cancellation.clone());
        Ok(cancellation)
    }

    fn finish(&self, game_slug: &str) {
        if let Ok(mut jobs) = self.cancellations.lock() {
            jobs.remove(game_slug);
        }
    }

    fn cancel(&self, game_slug: &str) -> Result<(), String> {
        let jobs = self
            .cancellations
            .lock()
            .map_err(|_| "the installation manager is unavailable".to_string())?;
        let cancellation = jobs
            .get(game_slug)
            .ok_or("no active installation was found for this game")?;
        cancellation.store(true, Ordering::Relaxed);
        Ok(())
    }
}

pub fn validate_game_slug(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 120
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err("invalid game slug".into());
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Result<PathBuf, String> {
    if value.is_empty() || value.starts_with('/') || value.starts_with('\\') || value.contains(':')
    {
        return Err(format!("unsafe installation path: {value}"));
    }
    let normalized = value.replace('\\', "/");
    let path = PathBuf::from(normalized);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe installation path: {value}"));
    }
    Ok(path)
}

fn validate_plan(plan: &DistributionPlan) -> Result<(), String> {
    validate_game_slug(&plan.game_slug)?;
    if plan.release.id != plan.manifest.release_id {
        return Err("manifest release does not match the resolved release".into());
    }
    if plan.release.artifact_id != plan.manifest.artifact_id
        || plan.release.artifact_id != plan.download.artifact_id
    {
        return Err("artifact identifiers do not match".into());
    }
    if plan.release.sha256 != plan.download.sha256 {
        return Err("artifact checksums do not match".into());
    }
    if plan.release.manifest_schema_version != "1" || plan.manifest.schema_version != "1" {
        return Err("unsupported install manifest version".into());
    }
    if plan.download.sha256.len() != 64
        || !plan
            .download
            .sha256
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
    {
        return Err("invalid artifact SHA-256".into());
    }
    let url = url::Url::parse(&plan.download.url)
        .map_err(|error| format!("invalid artifact URL: {error}"))?;
    if url.scheme() != "https" {
        return Err("artifact downloads must use HTTPS".into());
    }
    safe_relative_path(&plan.manifest.entrypoint)?;
    if let Some(directory) = &plan.manifest.working_directory {
        safe_relative_path(directory)?;
    }
    for executable in &plan.manifest.executables {
        safe_relative_path(executable)?;
    }
    Ok(())
}

fn app_data_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| format!("could not resolve application data directory: {error}"))
}

fn read_json_or_default<T>(path: &Path) -> Result<T, String>
where
    T: DeserializeOwned + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let bytes =
        fs::read(path).map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("could not parse {}: {error}", path.display()))
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
    }
    let temporary = path.with_extension("tmp");
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("could not serialize local state: {error}"))?;
    {
        let mut file = File::create(&temporary)
            .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
        file.write_all(&bytes)
            .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
        file.sync_all()
            .map_err(|error| format!("could not sync {}: {error}", temporary.display()))?;
    }
    let backup = path.with_extension("bak");
    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|error| format!("could not clear {}: {error}", backup.display()))?;
    }
    if path.exists() {
        fs::rename(path, &backup)
            .map_err(|error| format!("could not stage {}: {error}", path.display()))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if backup.exists() {
            let _ = fs::rename(&backup, path);
        }
        return Err(format!("could not finalize {}: {error}", path.display()));
    }
    if backup.exists() {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn installation_log_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?.join(INSTALLATION_LOG_FILE))
}

fn append_diagnostic_at(path: &Path, event: &InstallationDiagnosticEvent) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create diagnostics directory: {error}"))?;
    }
    if path.exists() {
        let sanitized = read_diagnostics_at(path)?
            .into_iter()
            .map(|event| serde_json::to_string(&event))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("could not sanitize diagnostics: {error}"))?;
        let contents = if sanitized.is_empty() {
            String::new()
        } else {
            format!("{}\n", sanitized.join("\n"))
        };
        fs::write(path, contents)
            .map_err(|error| format!("could not sanitize diagnostics: {error}"))?;
    }
    if fs::metadata(path)
        .map(|metadata| metadata.len() >= MAX_INSTALLATION_LOG_BYTES)
        .unwrap_or(false)
    {
        let previous = path.with_extension("previous.log");
        if previous.exists() {
            fs::remove_file(&previous)
                .map_err(|error| format!("could not rotate diagnostics: {error}"))?;
        }
        fs::rename(path, previous)
            .map_err(|error| format!("could not rotate diagnostics: {error}"))?;
    }
    let line = serde_json::to_string(event)
        .map_err(|error| format!("could not serialize diagnostics: {error}"))?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("could not open diagnostics: {error}"))?;
    writeln!(file, "{line}").map_err(|error| format!("could not write diagnostics: {error}"))
}

fn read_diagnostics_at(path: &Path) -> Result<Vec<InstallationDiagnosticEvent>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read installation diagnostics: {error}"))?;
    let mut events = contents
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect::<Vec<_>>();
    if events.len() > MAX_DIAGNOSTIC_EVENTS {
        events.drain(..events.len() - MAX_DIAGNOSTIC_EVENTS);
    }
    Ok(events)
}

fn classify_install_error(error: &str) -> &'static str {
    if error == DOWNLOAD_AUTHORIZATION_EXPIRED {
        "DOWNLOAD_AUTHORIZATION_EXPIRED"
    } else if error.contains("integrity") || error.contains("checksum") {
        "INTEGRITY_FAILURE"
    } else if error.contains("manifest") || error.contains("entrypoint") {
        "INVALID_ARTIFACT_MANIFEST"
    } else if error.contains("archive") || error.contains("ZIP") {
        "INVALID_ARCHIVE"
    } else if error.contains("download") || error.contains("artifact server") {
        "DOWNLOAD_FAILED"
    } else if error.contains("cancelled") {
        "CANCELLED"
    } else {
        "INSTALLATION_FAILED"
    }
}

fn append_install_diagnostic(
    app: &AppHandle,
    plan: &DistributionPlan,
    event: &str,
    error_code: Option<&str>,
) {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let diagnostic = InstallationDiagnosticEvent {
        timestamp,
        game_slug: plan.game_slug.clone(),
        event: event.to_string(),
        release_id: Some(plan.release.id.clone()),
        artifact_id: Some(plan.release.artifact_id.clone()),
        version: Some(plan.release.version.clone()),
        total_bytes: Some(plan.download.total_size_bytes.clone()),
        error_code: error_code.map(str::to_string),
    };
    if let Ok(path) = installation_log_path(app) {
        let _ = append_diagnostic_at(&path, &diagnostic);
    }
}

fn preferences_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?.join(PREFERENCES_FILE))
}

fn registry_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?.join(REGISTRY_FILE))
}

fn read_registry_at(path: &Path) -> Result<Vec<InstalledGame>, String> {
    read_json_or_default(path)
}

fn write_registry_at(path: &Path, registry: &[InstalledGame]) -> Result<(), String> {
    write_json_atomic(path, &registry)
}

fn installation_status(game: &InstalledGame) -> InstallationStatus {
    let root = PathBuf::from(&game.install_directory);
    if !root.is_absolute() || !root.is_dir() {
        return InstallationStatus::RepairNeeded;
    }
    let Ok(entrypoint) = safe_relative_path(&game.entrypoint) else {
        return InstallationStatus::RepairNeeded;
    };
    if !root.join(entrypoint).is_file() {
        return InstallationStatus::RepairNeeded;
    }
    if let Some(directory) = &game.working_directory {
        let Ok(directory) = safe_relative_path(directory) else {
            return InstallationStatus::RepairNeeded;
        };
        if !root.join(directory).is_dir() {
            return InstallationStatus::RepairNeeded;
        }
    }
    InstallationStatus::Installed
}

fn reconcile_installations_at(path: &Path) -> Result<Vec<InstalledGame>, String> {
    let mut registry = read_registry_at(path)?;
    let mut changed = false;
    for game in &mut registry {
        let status = installation_status(game);
        if game.status != status {
            game.status = status;
            changed = true;
        }
    }
    if changed {
        write_registry_at(path, &registry)?;
    }
    Ok(registry)
}

fn resolve_launch_plan(game: &InstalledGame) -> Result<LaunchPlan, String> {
    if installation_status(game) != InstallationStatus::Installed {
        return Err("the installed game needs repair".into());
    }
    let root = PathBuf::from(&game.install_directory);
    let executable = root.join(safe_relative_path(&game.entrypoint)?);
    let working_directory = match &game.working_directory {
        Some(directory) => root.join(safe_relative_path(directory)?),
        None => root,
    };
    Ok(LaunchPlan {
        executable,
        working_directory,
        arguments: game.launch_arguments.clone(),
        environment: game.environment.clone(),
    })
}

fn default_install_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?.join("games"))
}

fn configured_install_root(app: &AppHandle) -> Result<PathBuf, String> {
    let preferences: StoredPreferences = read_json_or_default(&preferences_path(app)?)?;
    match preferences.install_directory {
        Some(directory) => {
            let path = PathBuf::from(directory);
            if !path.is_absolute() || path.parent().is_none() {
                return Err("choose an absolute folder below the filesystem root".into());
            }
            Ok(path)
        }
        None => default_install_root(app),
    }
}

fn emit_progress(
    app: &AppHandle,
    plan: &DistributionPlan,
    title: &str,
    phase: &str,
    downloaded_bytes: u64,
    total_bytes: u64,
    error: Option<String>,
) {
    let _ = app.emit(
        "installation-progress",
        InstallationProgress {
            game_slug: plan.game_slug.clone(),
            title: title.to_string(),
            phase: phase.to_string(),
            downloaded_bytes,
            total_bytes,
            version: Some(plan.release.version.clone()),
            error,
        },
    );
}

async fn download_artifact(
    app: &AppHandle,
    title: &str,
    plan: &DistributionPlan,
    cancellation: &AtomicBool,
) -> Result<PathBuf, String> {
    let total = plan
        .download
        .total_size_bytes
        .parse::<u64>()
        .map_err(|_| "invalid artifact size".to_string())?;
    let download_directory = app_data_directory(app)?.join("downloads");
    tokio::fs::create_dir_all(&download_directory)
        .await
        .map_err(|error| format!("could not create download directory: {error}"))?;
    let part_path = partial_download_path(app, &plan.download.artifact_id)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("could not initialize downloader: {error}"))?;
    download_to_file(
        &client,
        &plan.download.url,
        &part_path,
        total,
        plan.download.etag.as_deref(),
        cancellation,
        |downloaded| {
            emit_progress(app, plan, title, "downloading", downloaded, total, None);
        },
    )
    .await?;
    Ok(part_path)
}

fn partial_download_path(app: &AppHandle, artifact_id: &str) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?
        .join("downloads")
        .join(format!("{artifact_id}.part")))
}

fn saved_download_progress(app: &AppHandle, plan: &DistributionPlan) -> u64 {
    partial_download_path(app, &plan.download.artifact_id)
        .ok()
        .and_then(|path| fs::metadata(path).ok())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
        .min(plan.download.total_size_bytes.parse::<u64>().unwrap_or(0))
}

async fn download_to_file<F>(
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    total: u64,
    etag: Option<&str>,
    cancellation: &AtomicBool,
    mut on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64),
{
    let mut retries_without_progress = 0;
    let mut restarted_invalid_range = false;

    'request: loop {
        ensure_not_cancelled(cancellation)?;
        let mut existing = partial_file_size(part_path).await;
        if existing > total {
            truncate_partial_file(part_path).await?;
            existing = 0;
        }
        if existing == total {
            on_progress(existing);
            return Ok(());
        }

        let mut request = client.get(url);
        if existing > 0 {
            request = request.header(reqwest::header::RANGE, format!("bytes={existing}-"));
            if let Some(etag) = etag {
                request = request.header(reqwest::header::IF_RANGE, etag);
            }
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(_) => {
                retry_or_fail(&mut retries_without_progress, false, cancellation).await?;
                continue;
            }
        };
        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
        ) {
            return Err(DOWNLOAD_AUTHORIZATION_EXPIRED.into());
        }
        if is_transient_download_status(status) {
            retry_or_fail(&mut retries_without_progress, false, cancellation).await?;
            continue;
        }
        if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE && existing > 0 {
            if restarted_invalid_range {
                return Err("artifact server repeatedly rejected the saved range".into());
            }
            truncate_partial_file(part_path).await?;
            restarted_invalid_range = true;
            retries_without_progress = 0;
            continue;
        }
        if !status.is_success() {
            return Err(format!("artifact server returned {status}"));
        }

        let resumed = existing > 0 && status == reqwest::StatusCode::PARTIAL_CONTENT;
        validate_download_response(&response, existing, total, etag, resumed)?;
        let mut downloaded = if resumed { existing } else { 0 };
        let request_start = downloaded;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(part_path)
            .await
            .map_err(|error| format!("could not open partial download: {error}"))?;
        let mut stream = response.bytes_stream();
        on_progress(downloaded);
        while let Some(chunk) = stream.next().await {
            ensure_not_cancelled(cancellation)?;
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    file.flush()
                        .await
                        .map_err(|error| format!("could not flush artifact: {error}"))?;
                    retry_or_fail(
                        &mut retries_without_progress,
                        downloaded > request_start,
                        cancellation,
                    )
                    .await?;
                    continue 'request;
                }
            };
            file.write_all(&chunk)
                .await
                .map_err(|error| format!("could not save artifact: {error}"))?;
            downloaded = downloaded
                .checked_add(chunk.len() as u64)
                .ok_or("artifact download size overflow")?;
            if downloaded > total {
                return Err(format!(
                    "artifact size mismatch: expected {total} bytes, received more"
                ));
            }
            on_progress(downloaded);
        }
        file.flush()
            .await
            .map_err(|error| format!("could not flush artifact: {error}"))?;
        if downloaded == total {
            return Ok(());
        }
        retry_or_fail(
            &mut retries_without_progress,
            downloaded > request_start,
            cancellation,
        )
        .await?;
    }
}

fn ensure_not_cancelled(cancellation: &AtomicBool) -> Result<(), String> {
    if cancellation.load(Ordering::Relaxed) {
        Err("installation cancelled".into())
    } else {
        Ok(())
    }
}

async fn partial_file_size(path: &Path) -> u64 {
    tokio::fs::metadata(path)
        .await
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

async fn truncate_partial_file(path: &Path) -> Result<(), String> {
    tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .await
        .map(|_| ())
        .map_err(|error| format!("could not reset partial download: {error}"))
}

fn is_transient_download_status(status: reqwest::StatusCode) -> bool {
    matches!(
        status,
        reqwest::StatusCode::REQUEST_TIMEOUT
            | reqwest::StatusCode::TOO_MANY_REQUESTS
            | reqwest::StatusCode::INTERNAL_SERVER_ERROR
            | reqwest::StatusCode::BAD_GATEWAY
            | reqwest::StatusCode::SERVICE_UNAVAILABLE
            | reqwest::StatusCode::GATEWAY_TIMEOUT
    )
}

fn parse_content_range(value: &str) -> Option<(u64, u64, u64)> {
    let value = value.strip_prefix("bytes ")?;
    let (range, total) = value.split_once('/')?;
    let (start, end) = range.split_once('-')?;
    Some((start.parse().ok()?, end.parse().ok()?, total.parse().ok()?))
}

fn validate_download_response(
    response: &reqwest::Response,
    existing: u64,
    total: u64,
    etag: Option<&str>,
    resumed: bool,
) -> Result<(), String> {
    if resumed {
        let content_range = response
            .headers()
            .get(reqwest::header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(parse_content_range)
            .ok_or("artifact server returned an invalid Content-Range")?;
        if content_range.0 != existing
            || content_range.1 < content_range.0
            || content_range.1 >= total
            || content_range.2 != total
        {
            return Err("artifact server returned an unexpected Content-Range".into());
        }
        if let Some(length) = response.content_length() {
            let expected_length = content_range.1 - content_range.0 + 1;
            if length != expected_length {
                return Err("artifact partial response length does not match its range".into());
            }
        }
        if let Some(expected_etag) = etag {
            let response_etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|value| value.to_str().ok())
                .ok_or("artifact partial response is missing its ETag")?;
            if response_etag != expected_etag {
                return Err("artifact ETag changed while resuming the download".into());
            }
        }
    } else if let Some(length) = response.content_length() {
        if length != total {
            return Err("artifact response length does not match its declaration".into());
        }
    }
    Ok(())
}

async fn retry_or_fail(
    retries_without_progress: &mut u32,
    made_progress: bool,
    cancellation: &AtomicBool,
) -> Result<(), String> {
    if made_progress {
        *retries_without_progress = 0;
    } else {
        *retries_without_progress += 1;
    }
    if *retries_without_progress > MAX_TRANSIENT_DOWNLOAD_RETRIES {
        return Err(DOWNLOAD_INTERRUPTED.into());
    }
    wait_before_retry(*retries_without_progress, cancellation).await
}

async fn wait_before_retry(attempt: u32, cancellation: &AtomicBool) -> Result<(), String> {
    let exponent = attempt.saturating_sub(1).min(10);
    let cap = DOWNLOAD_RETRY_BASE_DELAY_MS
        .saturating_mul(1_u64 << exponent)
        .min(DOWNLOAD_RETRY_MAX_DELAY_MS);
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    let mut remaining = if cfg!(test) { 0 } else { seed % (cap + 1) };
    while remaining > 0 {
        ensure_not_cancelled(cancellation)?;
        let slice = remaining.min(100);
        tokio::time::sleep(Duration::from_millis(slice)).await;
        remaining -= slice;
    }
    ensure_not_cancelled(cancellation)
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path)
        .map_err(|error| format!("could not open artifact for verification: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 128];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("could not verify artifact: {error}"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_archive_checksum(path: &Path, expected: &str) -> Result<(), String> {
    if sha256_file(path)? != expected {
        return Err("artifact integrity verification failed".into());
    }
    Ok(())
}

fn extract_zip(archive_path: &Path, destination: &Path, maximum_size: u64) -> Result<(), String> {
    let file = File::open(archive_path)
        .map_err(|error| format!("could not open artifact archive: {error}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| format!("artifact is not a valid ZIP archive: {error}"))?;
    if archive.len() > MAX_ARCHIVE_FILES {
        return Err("artifact contains too many files".into());
    }
    let mut expanded_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("could not read ZIP entry: {error}"))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("symbolic links are not allowed in game artifacts".into());
        }
        let enclosed = entry
            .enclosed_name()
            .ok_or("artifact contains an unsafe path")?
            .to_path_buf();
        let relative = safe_relative_path(&enclosed.to_string_lossy())?;
        let output = destination.join(relative);
        expanded_size = expanded_size
            .checked_add(entry.size())
            .ok_or("artifact expanded size overflow")?;
        if expanded_size > maximum_size {
            return Err("artifact exceeds its declared installed size".into());
        }
        if entry.is_dir() {
            fs::create_dir_all(&output)
                .map_err(|error| format!("could not create {}: {error}", output.display()))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("could not create {}: {error}", parent.display()))?;
        }
        let mut output_file = File::create(&output)
            .map_err(|error| format!("could not create {}: {error}", output.display()))?;
        std::io::copy(&mut entry, &mut output_file)
            .map_err(|error| format!("could not extract {}: {error}", output.display()))?;
    }
    Ok(())
}

fn extract_and_validate_archive(
    archive_path: &Path,
    destination: &Path,
    maximum_size: u64,
    entrypoint: &str,
) -> Result<(), String> {
    extract_zip(archive_path, destination, maximum_size)?;
    let entrypoint = safe_relative_path(entrypoint)?;
    if !destination.join(entrypoint).is_file() {
        return Err("artifact does not contain the declared entrypoint".into());
    }
    Ok(())
}

fn activate_staged_installation(
    stage: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<(), String> {
    if backup.exists() {
        fs::remove_dir_all(backup)
            .map_err(|error| format!("could not clear installation backup: {error}"))?;
    }
    if destination.exists() {
        fs::rename(destination, backup)
            .map_err(|error| format!("could not stage existing installation: {error}"))?;
    }
    if let Err(error) = fs::rename(stage, destination) {
        if backup.exists() {
            let _ = fs::rename(backup, destination);
        }
        return Err(format!("could not activate installation: {error}"));
    }
    if backup.exists() {
        let _ = fs::remove_dir_all(backup);
    }
    Ok(())
}

fn install_archive_at_with_callback<F>(
    install_root: &Path,
    registry_file: &Path,
    title: &str,
    plan: &DistributionPlan,
    archive_path: &Path,
    on_extracted: F,
) -> Result<InstalledGame, String>
where
    F: FnOnce(),
{
    fs::create_dir_all(install_root)
        .map_err(|error| format!("could not create installation root: {error}"))?;
    let stage = install_root.join(format!(".{}.staging", plan.game_slug));
    let destination = install_root.join(&plan.game_slug);
    let backup = install_root.join(format!(".{}.backup", plan.game_slug));
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .map_err(|error| format!("could not clear staging directory: {error}"))?;
    }
    fs::create_dir_all(&stage)
        .map_err(|error| format!("could not create staging directory: {error}"))?;
    let declared_size = plan
        .release
        .installed_size_bytes
        .parse::<u64>()
        .map_err(|_| "invalid installed size".to_string())?;
    let maximum_size = declared_size.saturating_add(declared_size / 20).max(1);
    if let Err(error) = extract_and_validate_archive(
        archive_path,
        &stage,
        maximum_size,
        &plan.manifest.entrypoint,
    ) {
        let _ = fs::remove_dir_all(&stage);
        return Err(error);
    }
    on_extracted();
    activate_staged_installation(&stage, &destination, &backup)?;
    let installed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock is invalid".to_string())?
        .as_secs()
        .to_string();
    let installed = InstalledGame {
        game_slug: plan.game_slug.clone(),
        title: title.to_string(),
        version: plan.release.version.clone(),
        release_id: plan.release.id.clone(),
        release_number: plan.release.release_number,
        artifact_id: plan.release.artifact_id.clone(),
        installed_size_bytes: plan.release.installed_size_bytes.clone(),
        install_directory: destination.to_string_lossy().into_owned(),
        entrypoint: plan.manifest.entrypoint.clone(),
        installed_at,
        status: InstallationStatus::Installed,
        launch_arguments: plan.manifest.launch_arguments.clone(),
        working_directory: plan.manifest.working_directory.clone(),
        environment: plan.manifest.environment.clone(),
    };
    let mut registry = read_registry_at(registry_file)?;
    registry.retain(|item| item.game_slug != installed.game_slug);
    registry.push(installed.clone());
    write_registry_at(registry_file, &registry)?;
    Ok(installed)
}

#[cfg(test)]
fn install_archive_at(
    install_root: &Path,
    registry_file: &Path,
    title: &str,
    plan: &DistributionPlan,
    archive_path: &Path,
) -> Result<InstalledGame, String> {
    install_archive_at_with_callback(
        install_root,
        registry_file,
        title,
        plan,
        archive_path,
        || {},
    )
}

fn install_archive(
    app: &AppHandle,
    title: &str,
    plan: &DistributionPlan,
    archive_path: &Path,
) -> Result<InstalledGame, String> {
    let install_root = configured_install_root(app)?;
    install_archive_at_with_callback(
        &install_root,
        &registry_path(app)?,
        title,
        plan,
        archive_path,
        || emit_progress(app, plan, title, "installing", 1, 1, None),
    )
}

async fn install_game_inner(
    app: &AppHandle,
    title: &str,
    plan: &DistributionPlan,
    cancellation: &AtomicBool,
) -> Result<InstalledGame, String> {
    validate_plan(plan)?;
    let total = plan.download.total_size_bytes.parse::<u64>().unwrap_or(0);
    let archive = download_artifact(app, title, plan, cancellation).await?;
    emit_progress(app, plan, title, "verifying", total, total, None);
    let archive_for_hash = archive.clone();
    let expected_hash = plan.download.sha256.clone();
    let verification = tokio::task::spawn_blocking(move || {
        verify_archive_checksum(&archive_for_hash, &expected_hash)
    })
    .await
    .map_err(|error| format!("artifact verification task failed: {error}"))?;
    if let Err(error) = verification {
        let _ = tokio::fs::remove_file(&archive).await;
        return Err(error);
    }
    if cancellation.load(Ordering::Relaxed) {
        return Err("installation cancelled".into());
    }
    emit_progress(app, plan, title, "extracting", total, total, None);
    let app_for_install = app.clone();
    let title_for_install = title.to_string();
    let plan_for_install = plan.clone();
    let archive_for_install = archive.clone();
    let installed = tokio::task::spawn_blocking(move || {
        install_archive(
            &app_for_install,
            &title_for_install,
            &plan_for_install,
            &archive_for_install,
        )
    })
    .await
    .map_err(|error| format!("installation task failed: {error}"))??;
    let _ = tokio::fs::remove_file(&archive).await;
    Ok(installed)
}

#[tauri::command]
pub async fn install_game(
    app: AppHandle,
    manager: tauri::State<'_, InstallationManager>,
    title: String,
    plan: DistributionPlan,
) -> Result<InstalledGame, InstallCommandError> {
    let cancellation = manager
        .begin(&plan.game_slug)
        .map_err(InstallCommandError::from_message)?;
    append_install_diagnostic(&app, &plan, "STARTED", None);
    let result = install_game_inner(&app, &title, &plan, &cancellation).await;
    manager.finish(&plan.game_slug);
    match &result {
        Ok(_) => {
            append_install_diagnostic(&app, &plan, "INSTALLED", None);
            emit_progress(&app, &plan, &title, "installed", 1, 1, None)
        }
        Err(error) if error == "installation cancelled" => {
            append_install_diagnostic(&app, &plan, "CANCELLED", Some("CANCELLED"));
            emit_progress(&app, &plan, &title, "cancelled", 0, 0, None)
        }
        Err(error)
            if matches!(
                error.as_str(),
                DOWNLOAD_AUTHORIZATION_EXPIRED | DOWNLOAD_INTERRUPTED
            ) =>
        {
            let error_code = if error == DOWNLOAD_AUTHORIZATION_EXPIRED {
                "DOWNLOAD_AUTHORIZATION_EXPIRED"
            } else {
                "DOWNLOAD_INTERRUPTED"
            };
            append_install_diagnostic(&app, &plan, "DOWNLOAD_RECOVERY_REQUIRED", Some(error_code));
            let total = plan.download.total_size_bytes.parse::<u64>().unwrap_or(0);
            emit_progress(
                &app,
                &plan,
                &title,
                "downloading",
                saved_download_progress(&app, &plan),
                total,
                None,
            )
        }
        Err(error) => {
            append_install_diagnostic(&app, &plan, "FAILED", Some(classify_install_error(error)));
            emit_progress(&app, &plan, &title, "failed", 0, 0, Some(error.clone()))
        }
    }
    result.map_err(InstallCommandError::from_message)
}

#[tauri::command]
pub fn cancel_installation(
    manager: tauri::State<'_, InstallationManager>,
    game_slug: String,
) -> Result<(), String> {
    validate_game_slug(&game_slug)?;
    manager.cancel(&game_slug)
}

#[tauri::command]
pub fn list_installations(app: AppHandle) -> Result<Vec<InstalledGame>, String> {
    reconcile_installations_at(&registry_path(&app)?)
}

#[tauri::command]
pub fn launch_game(app: AppHandle, game_slug: String) -> Result<(), String> {
    validate_game_slug(&game_slug)?;
    let registry = reconcile_installations_at(&registry_path(&app)?)?;
    let game = registry
        .into_iter()
        .find(|item| item.game_slug == game_slug)
        .ok_or("game is not installed")?;
    let launch = resolve_launch_plan(&game)?;
    let mut command = Command::new(launch.executable);
    command
        .args(launch.arguments)
        .envs(launch.environment)
        .current_dir(launch.working_directory)
        .spawn()
        .map_err(|error| format!("could not launch game: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn get_installation_preferences(app: AppHandle) -> Result<InstallationPreferences, String> {
    let stored: StoredPreferences = read_json_or_default(&preferences_path(&app)?)?;
    Ok(InstallationPreferences {
        install_directory: stored.install_directory,
        default_install_directory: default_install_root(&app)?.to_string_lossy().into_owned(),
    })
}

#[tauri::command]
pub fn get_installation_diagnostics(app: AppHandle) -> Result<InstallationDiagnostics, String> {
    Ok(InstallationDiagnostics {
        app_version: env!("CARGO_PKG_VERSION"),
        events: read_diagnostics_at(&installation_log_path(&app)?)?,
    })
}

#[tauri::command]
pub fn set_installation_preferences(
    app: AppHandle,
    install_directory: Option<String>,
) -> Result<InstallationPreferences, String> {
    let install_directory = install_directory
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(directory) = &install_directory {
        let path = PathBuf::from(directory);
        if !path.is_absolute() || path.parent().is_none() {
            return Err("choose an absolute folder below the filesystem root".into());
        }
        fs::create_dir_all(&path)
            .map_err(|error| format!("could not create installation directory: {error}"))?;
    }
    write_json_atomic(
        &preferences_path(&app)?,
        &StoredPreferences {
            install_directory: install_directory.clone(),
        },
    )?;
    Ok(InstallationPreferences {
        install_directory,
        default_install_directory: default_install_root(&app)?.to_string_lossy().into_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::thread;

    fn test_plan(entrypoint: &str, installed_size: u64) -> DistributionPlan {
        let sha256 = "0".repeat(64);
        DistributionPlan {
            game_slug: "test-game".into(),
            release: ReleaseSummary {
                id: "release-1".into(),
                version: "1.0.0".into(),
                release_number: 1,
                published_at: "2026-08-19T00:00:00Z".into(),
                artifact_id: "artifact-1".into(),
                target: ReleaseTarget {
                    platform: "WINDOWS".into(),
                    architecture: "X86_64".into(),
                },
                compressed_size_bytes: "1".into(),
                installed_size_bytes: installed_size.to_string(),
                sha256: sha256.clone(),
                manifest_schema_version: "1".into(),
            },
            manifest: InstallManifest {
                schema_version: "1".into(),
                release_id: "release-1".into(),
                artifact_id: "artifact-1".into(),
                entrypoint: entrypoint.into(),
                launch_arguments: Vec::new(),
                working_directory: None,
                executables: vec![entrypoint.into()],
                environment: HashMap::new(),
            },
            download: DownloadAuthorization {
                artifact_id: "artifact-1".into(),
                url: "https://downloads.example.test/artifact.zip".into(),
                expires_at: "2026-08-19T01:00:00Z".into(),
                total_size_bytes: "1".into(),
                sha256,
                etag: None,
            },
        }
    }

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

    #[test]
    fn rejects_traversal_and_absolute_manifest_paths() {
        assert!(safe_relative_path("game.exe").is_ok());
        assert!(safe_relative_path("bin/game.exe").is_ok());
        assert!(safe_relative_path("../game.exe").is_err());
        assert!(safe_relative_path("bin/../../game.exe").is_err());
        assert!(safe_relative_path("C:\\game.exe").is_err());
        assert!(safe_relative_path("/game.exe").is_err());
    }

    #[test]
    fn accepts_only_filesystem_safe_game_slugs() {
        assert!(validate_game_slug("capyvarias-2").is_ok());
        assert!(validate_game_slug("../../windows").is_err());
        assert!(validate_game_slug("").is_err());
    }

    #[test]
    fn writes_local_state_atomically() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.json");
        write_json_atomic(&path, &vec!["one", "two"]).unwrap();
        let value: Vec<String> = read_json_or_default(&path).unwrap();
        assert_eq!(value, vec!["one", "two"]);
        assert!(!path.with_extension("tmp").exists());
    }

    #[test]
    fn computes_sha256_for_download_verification() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.zip");
        fs::write(&path, b"manifold").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "4dd5b6e2bf3bfd4f6a273018b06a65680ef9631f8e0156e6ddd6a06ca0510172"
        );
    }

    #[test]
    fn diagnostics_store_stable_codes_without_sensitive_error_details() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join(INSTALLATION_LOG_FILE);
        let raw_error =
            "artifact download failed: https://storage.test/file?token=super-secret C:\\Users\\Player";
        fs::write(&path, format!("legacy log: {raw_error}\n")).unwrap();
        let event = InstallationDiagnosticEvent {
            timestamp: 1_776_000_000,
            game_slug: "test-game".into(),
            event: "FAILED".into(),
            release_id: Some("release-1".into()),
            artifact_id: Some("artifact-1".into()),
            version: Some("1.0.0".into()),
            total_bytes: Some("1024".into()),
            error_code: Some(classify_install_error(raw_error).into()),
        };

        append_diagnostic_at(&path, &event).unwrap();

        let stored = fs::read_to_string(&path).unwrap();
        assert!(!stored.contains("super-secret"));
        assert!(!stored.contains("storage.test"));
        assert!(!stored.contains("Users"));
        let diagnostics = read_diagnostics_at(&path).unwrap();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(
            diagnostics[0].error_code.as_deref(),
            Some("DOWNLOAD_FAILED")
        );
    }

    #[test]
    fn rejects_an_artifact_with_the_wrong_checksum() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.zip");
        fs::write(&path, b"manifold").unwrap();

        assert_eq!(
            verify_archive_checksum(&path, &"0".repeat(64)).unwrap_err(),
            "artifact integrity verification failed"
        );
    }

    #[tokio::test]
    async fn cancellation_stops_before_network_or_partial_file_creation() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        let client = reqwest::Client::new();
        let cancellation = AtomicBool::new(true);

        let error = download_to_file(
            &client,
            "http://127.0.0.1:1/artifact.zip",
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap_err();

        assert_eq!(error, "installation cancelled");
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn expired_authorization_preserves_the_partial_download_for_resume() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.to_ascii_lowercase().contains("range: bytes=4-"));
            stream
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let client = reqwest::Client::new();
        let cancellation = AtomicBool::new(false);

        let error = download_to_file(
            &client,
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            Some("artifact-v1"),
            &cancellation,
            |_| {},
        )
        .await
        .unwrap_err();

        server.join().unwrap();
        assert_eq!(error, DOWNLOAD_AUTHORIZATION_EXPIRED);
        assert_eq!(fs::read(&path).unwrap(), b"mani");
    }

    #[tokio::test]
    async fn resumes_an_interrupted_download_with_a_range_request() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.to_ascii_lowercase().contains("range: bytes=4-"));
            stream
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nConnection: close\r\n\r\nfold")
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let client = reqwest::Client::new();
        let cancellation = AtomicBool::new(false);
        download_to_file(
            &client,
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"manifold");
    }

    #[tokio::test]
    async fn automatically_retries_a_short_download_from_the_saved_partial_file() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            first
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = first.read(&mut request).unwrap();
            let first_request = String::from_utf8_lossy(&request[..count]);
            assert!(!first_request.to_ascii_lowercase().contains("range:"));
            first
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nmani")
                .unwrap();
            drop(first);

            let (mut second, _) = listener.accept().unwrap();
            second
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = second.read(&mut request).unwrap();
            let second_request = String::from_utf8_lossy(&request[..count]);
            assert!(second_request
                .to_ascii_lowercase()
                .contains("range: bytes=4-"));
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nConnection: close\r\n\r\nfold")
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        let client = reqwest::Client::new();
        let cancellation = AtomicBool::new(false);

        download_to_file(
            &client,
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap();
        server.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"manifold");
    }

    #[tokio::test]
    async fn rejects_a_partial_response_for_the_wrong_offset() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 5\r\nContent-Range: bytes 3-7/8\r\nConnection: close\r\n\r\nifold")
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let cancellation = AtomicBool::new(false);

        let error = download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap_err();

        server.join().unwrap();
        assert!(error.contains("unexpected Content-Range"));
        assert_eq!(fs::read(&path).unwrap(), b"mani");
    }

    #[tokio::test]
    async fn rejects_a_changed_etag_while_resuming() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            stream
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nETag: artifact-v2\r\nConnection: close\r\n\r\nfold")
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let cancellation = AtomicBool::new(false);

        let error = download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            Some("artifact-v1"),
            &cancellation,
            |_| {},
        )
        .await
        .unwrap_err();

        server.join().unwrap();
        assert!(error.contains("ETag changed"));
        assert_eq!(fs::read(&path).unwrap(), b"mani");
    }

    #[tokio::test]
    async fn safely_restarts_once_when_storage_rejects_the_saved_range() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let count = first.read(&mut request).unwrap();
            let request_text = String::from_utf8_lossy(&request[..count]);
            assert!(request_text
                .to_ascii_lowercase()
                .contains("range: bytes=4-"));
            first
                .write_all(b"HTTP/1.1 416 Range Not Satisfiable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();

            let (mut second, _) = listener.accept().unwrap();
            let count = second.read(&mut request).unwrap();
            let request_text = String::from_utf8_lossy(&request[..count]);
            assert!(!request_text.to_ascii_lowercase().contains("range:"));
            second
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\nmanifold",
                )
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let cancellation = AtomicBool::new(false);

        download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap();

        server.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"manifold");
    }

    #[tokio::test]
    async fn retries_transient_storage_failures_without_exposing_them() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut first, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = first.read(&mut request).unwrap();
            first
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .unwrap();

            let (mut second, _) = listener.accept().unwrap();
            let count = second.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.to_ascii_lowercase().contains("range: bytes=4-"));
            second
                .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nConnection: close\r\n\r\nfold")
                .unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        fs::write(&path, b"mani").unwrap();
        let cancellation = AtomicBool::new(false);

        download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap();

        server.join().unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"manifold");
    }

    #[tokio::test]
    async fn stops_after_bounded_transient_failures_without_progress() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            for _ in 0..=MAX_TRANSIENT_DOWNLOAD_RETRIES {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0_u8; 2048];
                let _ = stream.read(&mut request).unwrap();
                stream
                    .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                    .unwrap();
            }
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("artifact.part");
        let cancellation = AtomicBool::new(false);

        let error = download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/artifact.zip"),
            &path,
            8,
            None,
            &cancellation,
            |_| {},
        )
        .await
        .unwrap_err();

        server.join().unwrap();
        assert_eq!(error, DOWNLOAD_INTERRUPTED);
    }

    #[test]
    fn rejects_archive_entries_that_escape_the_staging_directory() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("malicious.zip");
        let file = File::create(&archive_path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file("../outside.exe", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"not executable").unwrap();
        archive.finish().unwrap();
        let destination = directory.path().join("staging");
        fs::create_dir_all(&destination).unwrap();
        assert!(extract_zip(&archive_path, &destination, 1024).is_err());
        assert!(!directory.path().join("outside.exe").exists());
    }

    #[test]
    fn rejects_a_malformed_zip_archive() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("malformed.zip");
        fs::write(&archive_path, b"not a zip archive").unwrap();
        let destination = directory.path().join("staging");
        fs::create_dir_all(&destination).unwrap();

        let error = extract_and_validate_archive(&archive_path, &destination, 1024, "game.exe")
            .unwrap_err();

        assert!(error.contains("not a valid ZIP archive"));
    }

    #[test]
    fn fresh_install_registers_only_after_entrypoint_validation() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("game.zip");
        write_zip(&archive_path, &[("game.exe", b"play")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let plan = test_plan("game.exe", 4);

        let installed =
            install_archive_at(&install_root, &registry, "Test Game", &plan, &archive_path)
                .unwrap();

        assert_eq!(installed.version, "1.0.0");
        assert_eq!(installed.release_number, 1);
        assert_eq!(installed.artifact_id, "artifact-1");
        assert_eq!(installed.installed_size_bytes, "4");
        assert_eq!(installed.status, InstallationStatus::Installed);
        assert!(install_root.join("test-game/game.exe").is_file());
        let stored: Vec<InstalledGame> = read_json_or_default(&registry).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].game_slug, "test-game");
        assert!(!install_root.join(".test-game.staging").exists());
        assert!(!install_root.join(".test-game.backup").exists());
    }

    #[test]
    fn restart_reconciles_and_preserves_a_valid_installation() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("game.zip");
        write_zip(&archive_path, &[("game.exe", b"play")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let plan = test_plan("game.exe", 4);
        install_archive_at(&install_root, &registry, "Test Game", &plan, &archive_path).unwrap();

        let restarted = reconcile_installations_at(&registry).unwrap();

        assert_eq!(restarted.len(), 1);
        assert_eq!(restarted[0].status, InstallationStatus::Installed);
        assert_eq!(restarted[0].release_id, "release-1");
        assert_eq!(restarted[0].artifact_id, "artifact-1");
    }

    #[test]
    fn missing_entrypoint_is_retained_as_repair_needed() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("game.zip");
        write_zip(&archive_path, &[("game.exe", b"play")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let plan = test_plan("game.exe", 4);
        install_archive_at(&install_root, &registry, "Test Game", &plan, &archive_path).unwrap();
        fs::remove_file(install_root.join("test-game/game.exe")).unwrap();

        let reconciled = reconcile_installations_at(&registry).unwrap();

        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].status, InstallationStatus::RepairNeeded);
        let persisted = read_registry_at(&registry).unwrap();
        assert_eq!(persisted[0].status, InstallationStatus::RepairNeeded);
    }

    #[test]
    fn launch_plan_uses_the_registered_entrypoint_and_working_directory() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("game.zip");
        write_zip(&archive_path, &[("bin/game.exe", b"play")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let mut plan = test_plan("bin/game.exe", 4);
        plan.manifest.working_directory = Some("bin".into());
        plan.manifest.launch_arguments = vec!["--safe-mode".into()];
        plan.manifest
            .environment
            .insert("MANIFOLD_TEST".into(), "1".into());
        let installed =
            install_archive_at(&install_root, &registry, "Test Game", &plan, &archive_path)
                .unwrap();

        let launch = resolve_launch_plan(&installed).unwrap();

        assert_eq!(
            launch.executable,
            install_root.join("test-game/bin/game.exe")
        );
        assert_eq!(launch.working_directory, install_root.join("test-game/bin"));
        assert_eq!(launch.arguments, vec!["--safe-mode"]);
        assert_eq!(launch.environment.get("MANIFOLD_TEST"), Some(&"1".into()));
    }

    #[test]
    fn reinstall_updates_one_registry_record_without_losing_playability() {
        let directory = tempfile::tempdir().unwrap();
        let first_archive = directory.path().join("game-v1.zip");
        let second_archive = directory.path().join("game-v2.zip");
        write_zip(&first_archive, &[("game.exe", b"old!")]);
        write_zip(&second_archive, &[("game.exe", b"new!")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let first = test_plan("game.exe", 4);
        install_archive_at(
            &install_root,
            &registry,
            "Test Game",
            &first,
            &first_archive,
        )
        .unwrap();
        let mut second = test_plan("game.exe", 4);
        second.release.id = "release-2".into();
        second.release.version = "2.0.0".into();
        second.release.release_number = 2;
        second.release.artifact_id = "artifact-2".into();
        second.manifest.release_id = "release-2".into();
        second.manifest.artifact_id = "artifact-2".into();
        second.download.artifact_id = "artifact-2".into();

        install_archive_at(
            &install_root,
            &registry,
            "Test Game",
            &second,
            &second_archive,
        )
        .unwrap();

        let stored = reconcile_installations_at(&registry).unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].version, "2.0.0");
        assert_eq!(stored[0].release_number, 2);
        assert_eq!(stored[0].artifact_id, "artifact-2");
        assert_eq!(stored[0].status, InstallationStatus::Installed);
        assert_eq!(
            fs::read(install_root.join("test-game/game.exe")).unwrap(),
            b"new!"
        );
        assert!(!install_root.join(".test-game.backup").exists());
    }

    #[test]
    fn installation_preferences_use_atomic_state_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("installation-preferences.json");
        let first = StoredPreferences {
            install_directory: Some("C:\\Games\\Manifold".into()),
        };
        let second = StoredPreferences {
            install_directory: Some("D:\\Games\\Manifold".into()),
        };

        write_json_atomic(&path, &first).unwrap();
        write_json_atomic(&path, &second).unwrap();

        let stored: StoredPreferences = read_json_or_default(&path).unwrap();
        assert_eq!(stored.install_directory, second.install_directory);
        assert!(!path.with_extension("tmp").exists());
        assert!(!path.with_extension("bak").exists());
    }

    #[test]
    fn missing_entrypoint_does_not_activate_or_register_installation() {
        let directory = tempfile::tempdir().unwrap();
        let archive_path = directory.path().join("game.zip");
        write_zip(&archive_path, &[("readme.txt", b"docs")]);
        let install_root = directory.path().join("games");
        let registry = directory.path().join("installations.json");
        let plan = test_plan("game.exe", 4);

        let error = install_archive_at(&install_root, &registry, "Test Game", &plan, &archive_path)
            .unwrap_err();

        assert_eq!(error, "artifact does not contain the declared entrypoint");
        assert!(!install_root.join("test-game").exists());
        assert!(!install_root.join(".test-game.staging").exists());
        assert!(!registry.exists());
    }

    #[test]
    fn failed_activation_restores_the_previous_working_installation() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("test-game");
        let backup = directory.path().join(".test-game.backup");
        let missing_stage = directory.path().join(".test-game.staging");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("old.exe"), b"working").unwrap();

        let error =
            activate_staged_installation(&missing_stage, &destination, &backup).unwrap_err();

        assert!(error.contains("could not activate installation"));
        assert_eq!(fs::read(destination.join("old.exe")).unwrap(), b"working");
        assert!(!backup.exists());
    }

    #[test]
    fn progress_events_keep_the_frontend_contract_stable() {
        let progress = InstallationProgress {
            game_slug: "test-game".into(),
            title: "Test Game".into(),
            phase: "failed".into(),
            downloaded_bytes: 4,
            total_bytes: 8,
            version: Some("1.0.0".into()),
            error: Some("network unavailable".into()),
        };

        let value = serde_json::to_value(progress).unwrap();
        assert_eq!(value["gameSlug"], "test-game");
        assert_eq!(value["downloadedBytes"], 4);
        assert_eq!(value["totalBytes"], 8);
        assert_eq!(value["phase"], "failed");
        assert_eq!(value["error"], "network unavailable");
    }
}
