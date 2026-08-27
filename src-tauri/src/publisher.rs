use futures_util::stream;
use reqwest::{
    header::{
        HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE,
        COOKIE, HOST, PROXY_AUTHORIZATION,
    },
    Client, Response, StatusCode,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File as SyncFile,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, Runtime};
use time::{format_description::well_known::Rfc3339, Duration, OffsetDateTime};
use tokio::{
    fs::File,
    io::{AsyncReadExt, BufReader},
};

use crate::{api_base_url, ApiState};

const MAX_ARCHIVE_FILES: usize = 100_000;
const MAX_INSTALLED_SIZE_BYTES: u64 = 200 * 1024 * 1024 * 1024;
const MAX_ENTRY_SIZE_BYTES: u64 = 100 * 1024 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 200;
const HASH_BUFFER_SIZE: usize = 128 * 1024;
const UPLOAD_BUFFER_SIZE: usize = 256 * 1024;
const AUTHORIZATION_SAFETY_WINDOW_SECONDS: i64 = 60;
const MAX_AUTHORIZATION_REFRESHES: u32 = 2;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl PublisherError {
    fn new(code: &str, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }

    fn invalid(message: impl Into<String>) -> Self {
        Self::new("INVALID_REQUEST", message, false)
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::new("SERVICE_UNAVAILABLE", message, true)
    }

    fn cancelled() -> Self {
        Self::new("UPLOAD_CANCELLED", "upload cancelled", true)
    }
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    message: Option<String>,
    action: Option<String>,
}

async fn publisher_api_json<T: DeserializeOwned>(response: Response) -> Result<T, PublisherError> {
    let status = response.status();
    if status.is_success() {
        return response.json().await.map_err(|_| {
            PublisherError::unavailable("Manifold API returned incompatible publisher data")
        });
    }

    let body = response.json::<ApiErrorBody>().await.ok();
    let message = body
        .as_ref()
        .and_then(|error| error.message.clone())
        .unwrap_or_else(|| format!("Manifold API returned {status}"));
    let message = match body.and_then(|error| error.action) {
        Some(action) => format!("{message}. {action}"),
        None => message,
    };
    Err(error_for_status(status, message))
}

fn error_for_status(status: StatusCode, message: impl Into<String>) -> PublisherError {
    let message = message.into();
    match status {
        StatusCode::UNAUTHORIZED => PublisherError::new("AUTHENTICATION_REQUIRED", message, false),
        StatusCode::FORBIDDEN => PublisherError::new("PERMISSION_DENIED", message, false),
        StatusCode::NOT_FOUND => PublisherError::new("NOT_FOUND", message, false),
        StatusCode::TOO_MANY_REQUESTS => PublisherError::new("RATE_LIMITED", message, true),
        status if status.is_server_error() => PublisherError::unavailable(message),
        _ => PublisherError::invalid(message),
    }
}

fn publisher_environment() -> String {
    std::env::var("MANIFOLD_APP_ENV").unwrap_or_else(|_| "production".into())
}

fn publisher_api_url(path: &str) -> Result<url::Url, String> {
    let environment = publisher_environment();
    let configured_origin = std::env::var("MANIFOLD_API_BASE_URL").ok();
    api_base_url(&environment, configured_origin.as_deref())?
        .join(path)
        .map_err(|error| format!("invalid API URL: {error}"))
}

async fn api_get<T: DeserializeOwned>(client: &Client, path: &str) -> Result<T, PublisherError> {
    let url = publisher_api_url(path).map_err(PublisherError::unavailable)?;
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| PublisherError::unavailable("could not reach the Manifold API"))?;
    publisher_api_json(response).await
}

async fn api_post<B: Serialize, T: DeserializeOwned>(
    client: &Client,
    path: &str,
    body: &B,
) -> Result<T, PublisherError> {
    let url = publisher_api_url(path).map_err(PublisherError::unavailable)?;
    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .json(body)
        .send()
        .await
        .map_err(|_| PublisherError::unavailable("could not reach the Manifold API"))?;
    publisher_api_json(response).await
}

async fn api_post_empty<T: DeserializeOwned>(
    client: &Client,
    path: &str,
) -> Result<T, PublisherError> {
    let url = publisher_api_url(path).map_err(PublisherError::unavailable)?;
    let response = client
        .post(url)
        .header(ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| PublisherError::unavailable("could not reach the Manifold API"))?;
    publisher_api_json(response).await
}

#[derive(Debug, Deserialize)]
struct StudiosEnvelope {
    studios: Vec<StudioApi>,
}

#[derive(Debug, Deserialize)]
struct StudioApi {
    id: String,
    slug: String,
    name: String,
    owner_id: String,
    description: Option<String>,
    logo_url: Option<String>,
    #[serde(default)]
    is_publisher: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherStudio {
    id: String,
    slug: String,
    name: String,
    owner_id: String,
    description: Option<String>,
    logo_url: Option<String>,
    is_publisher: bool,
}

impl From<StudioApi> for PublisherStudio {
    fn from(value: StudioApi) -> Self {
        Self {
            id: value.id,
            slug: value.slug,
            name: value.name,
            owner_id: value.owner_id,
            description: value.description,
            logo_url: value.logo_url,
            is_publisher: value.is_publisher,
        }
    }
}

#[derive(Debug, Deserialize)]
struct GamesEnvelope {
    games: Vec<GameApi>,
}

#[derive(Debug, Default, Deserialize)]
struct GameMediaApi {
    banner: Option<String>,
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GameApi {
    id: String,
    slug: String,
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    status: String,
    #[serde(default)]
    developer_name: String,
    #[serde(default)]
    media: GameMediaApi,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherGame {
    id: String,
    slug: String,
    title: String,
    description: String,
    status: String,
    developer_name: String,
    banner_url: Option<String>,
    icon_url: Option<String>,
}

impl From<GameApi> for PublisherGame {
    fn from(value: GameApi) -> Self {
        Self {
            id: value.id,
            slug: value.slug,
            title: value.title,
            description: value.description,
            status: value.status,
            developer_name: value.developer_name,
            banner_url: value.media.banner,
            icon_url: value.media.icon,
        }
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
struct PublisherReleaseApi {
    id: String,
    game_id: String,
    version: String,
    release_number: u64,
    status: String,
    #[serde(default)]
    release_notes: Option<String>,
    published_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublisherRelease {
    id: String,
    game_id: String,
    version: String,
    release_number: u64,
    status: String,
    release_notes: Option<String>,
    published_at: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<PublisherReleaseApi> for PublisherRelease {
    fn from(value: PublisherReleaseApi) -> Self {
        Self {
            id: value.id,
            game_id: value.game_id,
            version: value.version,
            release_number: value.release_number,
            status: value.status,
            release_notes: value.release_notes,
            published_at: value.published_at,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
struct CreateReleaseRequest<'a> {
    version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    release_notes: Option<&'a str>,
}

fn validate_slug(value: &str, kind: &str) -> Result<(), PublisherError> {
    if value.is_empty()
        || value.len() > 255
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '-' || character == '_'
        })
    {
        return Err(PublisherError::invalid(format!("invalid {kind} slug")));
    }
    Ok(())
}

#[tauri::command]
pub(crate) async fn list_publisher_studios(
    state: tauri::State<'_, ApiState>,
) -> Result<Vec<PublisherStudio>, PublisherError> {
    let client = state.client().map_err(PublisherError::unavailable)?;
    let envelope: StudiosEnvelope = api_get(&client, "studios").await?;
    Ok(envelope.studios.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub(crate) async fn list_studio_games(
    state: tauri::State<'_, ApiState>,
    studio_slug: String,
) -> Result<Vec<PublisherGame>, PublisherError> {
    validate_slug(&studio_slug, "studio")?;
    let client = state.client().map_err(PublisherError::unavailable)?;
    let envelope: GamesEnvelope = api_get(&client, &format!("studios/{studio_slug}/games")).await?;
    Ok(envelope.games.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub(crate) async fn create_release_draft(
    state: tauri::State<'_, ApiState>,
    game_slug: String,
    version: String,
    release_notes: Option<String>,
) -> Result<PublisherRelease, PublisherError> {
    validate_slug(&game_slug, "game")?;
    let version = version.trim();
    if version.is_empty() || version.len() > 50 {
        return Err(PublisherError::invalid(
            "release version must contain 1 to 50 characters",
        ));
    }
    let release_notes = release_notes
        .as_deref()
        .map(str::trim)
        .filter(|notes| !notes.is_empty());
    if release_notes.is_some_and(|notes| notes.len() > 100_000) {
        return Err(PublisherError::invalid(
            "release notes must contain at most 100000 characters",
        ));
    }

    let client = state.client().map_err(PublisherError::unavailable)?;
    let release: PublisherReleaseApi = api_post(
        &client,
        &format!("games/{game_slug}/releases"),
        &CreateReleaseRequest {
            version,
            release_notes,
        },
    )
    .await?;
    Ok(release.into())
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishArchiveInspection {
    archive_path: String,
    file_name: String,
    compressed_size_bytes: String,
    installed_size_bytes: String,
    sha256: String,
    executables: Vec<String>,
    suggested_entrypoint: String,
    suggested_working_directory: Option<String>,
}

#[derive(Debug)]
struct ArchiveInspection {
    archive_path: PathBuf,
    compressed_size_bytes: u64,
    installed_size_bytes: u64,
    sha256: String,
    files_by_folded_path: HashMap<String, String>,
    directories_by_folded_path: HashMap<String, String>,
    executables: Vec<String>,
}

impl ArchiveInspection {
    fn into_public(self) -> Result<PublishArchiveInspection, PublisherError> {
        let file_name = self
            .archive_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| PublisherError::invalid("archive file name is not valid UTF-8"))?
            .to_string();
        let suggested_entrypoint = self.executables.first().cloned().ok_or_else(|| {
            PublisherError::invalid("archive does not contain a Windows executable")
        })?;
        let suggested_working_directory = parent_path(&suggested_entrypoint);
        Ok(PublishArchiveInspection {
            archive_path: self.archive_path.to_string_lossy().into_owned(),
            file_name,
            compressed_size_bytes: self.compressed_size_bytes.to_string(),
            installed_size_bytes: self.installed_size_bytes.to_string(),
            sha256: self.sha256,
            executables: self.executables,
            suggested_entrypoint,
            suggested_working_directory,
        })
    }
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PublishManifest {
    pub schema_version: String,
    pub entrypoint: String,
    #[serde(default)]
    pub launch_arguments: Vec<String>,
    pub working_directory: Option<String>,
    #[serde(default)]
    pub executables: Vec<String>,
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ManifestDeclaration {
    pub schema_version: String,
    pub entrypoint: String,
    pub launch_arguments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_directory: Option<String>,
    pub executables: Vec<String>,
    pub environment: BTreeMap<String, String>,
}

impl From<ManifestDeclaration> for PublishManifest {
    fn from(value: ManifestDeclaration) -> Self {
        Self {
            schema_version: value.schema_version,
            entrypoint: value.entrypoint,
            launch_arguments: value.launch_arguments,
            working_directory: value.working_directory,
            executables: value.executables,
            environment: value.environment,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UploadDeclaration {
    platform: &'static str,
    architecture: &'static str,
    archive_format: &'static str,
    compressed_size_bytes: String,
    installed_size_bytes: String,
    sha256: String,
    manifest: ManifestDeclaration,
}

fn inspect_archive(path: &Path) -> Result<ArchiveInspection, PublisherError> {
    if !path.is_file() {
        return Err(PublisherError::invalid("archive does not exist"));
    }
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("zip"))
    {
        return Err(PublisherError::invalid(
            "the Windows MVP artifact must use the .zip extension",
        ));
    }

    let compressed_size_bytes = path
        .metadata()
        .map_err(|_| PublisherError::invalid("could not read archive metadata"))?
        .len();
    if compressed_size_bytes == 0 {
        return Err(PublisherError::invalid("archive must not be empty"));
    }

    let file =
        SyncFile::open(path).map_err(|_| PublisherError::invalid("could not open archive"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|_| PublisherError::invalid("invalid ZIP archive"))?;
    if archive.is_empty() {
        return Err(PublisherError::invalid("archive is empty"));
    }
    if archive.len() > MAX_ARCHIVE_FILES {
        return Err(PublisherError::invalid(format!(
            "archive contains more than {MAX_ARCHIVE_FILES} entries"
        )));
    }

    let mut installed_size_bytes = 0_u64;
    let mut paths = HashSet::new();
    let mut files_by_folded_path = HashMap::new();
    let mut directories_by_folded_path = HashMap::new();
    let mut executables = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| PublisherError::invalid("could not read ZIP entry"))?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(PublisherError::invalid(
                "symbolic links are not allowed in game artifacts",
            ));
        }
        entry
            .enclosed_name()
            .ok_or_else(|| PublisherError::invalid("archive contains an unsafe path"))?;
        let normalized = normalize_relative_path(entry.name().trim_end_matches('/'))?;
        let folded = normalized.to_ascii_lowercase();
        if !paths.insert(folded.clone()) {
            return Err(PublisherError::invalid(format!(
                "archive contains a duplicate Windows path: {normalized}"
            )));
        }

        if entry.is_dir() {
            directories_by_folded_path.insert(folded, normalized);
            continue;
        }
        if entry.size() > MAX_ENTRY_SIZE_BYTES {
            return Err(PublisherError::invalid(
                "archive contains an oversized entry",
            ));
        }
        installed_size_bytes = installed_size_bytes
            .checked_add(entry.size())
            .ok_or_else(|| PublisherError::invalid("installed size overflow"))?;
        if installed_size_bytes > MAX_INSTALLED_SIZE_BYTES {
            return Err(PublisherError::invalid(
                "archive exceeds the maximum installed size",
            ));
        }
        if entry.compressed_size() > 0
            && entry.size() / entry.compressed_size() > MAX_COMPRESSION_RATIO
        {
            return Err(PublisherError::invalid(
                "archive entry exceeds the maximum compression ratio",
            ));
        }

        let copied = std::io::copy(&mut entry, &mut std::io::sink())
            .map_err(|_| PublisherError::invalid("archive contains a corrupt ZIP entry"))?;
        if copied != entry.size() {
            return Err(PublisherError::invalid(
                "archive entry size does not match its declaration",
            ));
        }
        if normalized.to_ascii_lowercase().ends_with(".exe") {
            executables.push(normalized.clone());
        }
        files_by_folded_path.insert(folded, normalized);
    }

    if installed_size_bytes == 0 {
        return Err(PublisherError::invalid(
            "archive installed size must be greater than zero",
        ));
    }
    if installed_size_bytes / compressed_size_bytes > MAX_COMPRESSION_RATIO {
        return Err(PublisherError::invalid(
            "archive exceeds the maximum total compression ratio",
        ));
    }
    if executables.is_empty() {
        return Err(PublisherError::invalid(
            "archive does not contain a Windows executable",
        ));
    }
    executables.sort_by_key(|path| path.to_ascii_lowercase());

    Ok(ArchiveInspection {
        archive_path: path.to_path_buf(),
        compressed_size_bytes,
        installed_size_bytes,
        sha256: sha256_file(path)?,
        files_by_folded_path,
        directories_by_folded_path,
        executables,
    })
}

fn sha256_file(path: &Path) -> Result<String, PublisherError> {
    let mut file =
        SyncFile::open(path).map_err(|_| PublisherError::invalid("could not hash archive"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; HASH_BUFFER_SIZE];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| PublisherError::invalid("could not hash archive"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn normalize_relative_path(value: &str) -> Result<String, PublisherError> {
    if value.is_empty() || value.starts_with('/') || value.starts_with('\\') || value.contains(':')
    {
        return Err(PublisherError::invalid("unsafe artifact path"));
    }
    let normalized = value.replace('\\', "/");
    let path = Path::new(&normalized);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PublisherError::invalid("unsafe artifact path"));
    }
    Ok(normalized)
}

fn parent_path(value: &str) -> Option<String> {
    value.rsplit_once('/').map(|(parent, _)| parent.to_string())
}

fn build_declaration(
    inspection: &ArchiveInspection,
    manifest: PublishManifest,
) -> Result<UploadDeclaration, PublisherError> {
    if manifest.schema_version != "1" {
        return Err(PublisherError::invalid(
            "unsupported install manifest schema version",
        ));
    }
    let entrypoint = normalize_relative_path(&manifest.entrypoint)?;
    if !entrypoint.to_ascii_lowercase().ends_with(".exe") {
        return Err(PublisherError::invalid(
            "manifest entrypoint must be a Windows executable",
        ));
    }
    if !inspection
        .files_by_folded_path
        .contains_key(&entrypoint.to_ascii_lowercase())
    {
        return Err(PublisherError::invalid(
            "archive does not contain the declared entrypoint",
        ));
    }

    let working_directory = manifest
        .working_directory
        .as_deref()
        .map(normalize_relative_path)
        .transpose()?;
    if let Some(directory) = &working_directory {
        let folded = directory.to_ascii_lowercase();
        let prefix = format!("{folded}/");
        if !inspection.directories_by_folded_path.contains_key(&folded)
            && !inspection
                .files_by_folded_path
                .keys()
                .any(|path| path.starts_with(&prefix))
        {
            return Err(PublisherError::invalid(
                "archive does not contain the working directory",
            ));
        }
    }

    let mut executables = manifest
        .executables
        .iter()
        .map(|path| normalize_relative_path(path))
        .collect::<Result<Vec<_>, _>>()?;
    if executables.is_empty() {
        executables = inspection.executables.clone();
    }
    let mut seen = HashSet::new();
    executables.retain(|path| seen.insert(path.to_ascii_lowercase()));
    if !seen.contains(&entrypoint.to_ascii_lowercase()) {
        executables.insert(0, entrypoint.clone());
    }
    for executable in &executables {
        if !executable.to_ascii_lowercase().ends_with(".exe")
            || !inspection
                .files_by_folded_path
                .contains_key(&executable.to_ascii_lowercase())
        {
            return Err(PublisherError::invalid(
                "archive does not contain a declared executable",
            ));
        }
    }
    if manifest.launch_arguments.len() > 128
        || manifest
            .launch_arguments
            .iter()
            .any(|argument| argument.len() > 4096)
        || manifest.environment.len() > 128
        || manifest
            .environment
            .iter()
            .any(|(key, value)| key.is_empty() || key.len() > 255 || value.len() > 16_384)
    {
        return Err(PublisherError::invalid(
            "manifest arguments or environment exceed desktop safety limits",
        ));
    }

    Ok(UploadDeclaration {
        platform: "WINDOWS",
        architecture: "X86_64",
        archive_format: "ZIP",
        compressed_size_bytes: inspection.compressed_size_bytes.to_string(),
        installed_size_bytes: inspection.installed_size_bytes.to_string(),
        sha256: inspection.sha256.clone(),
        manifest: ManifestDeclaration {
            schema_version: "1".into(),
            entrypoint,
            launch_arguments: manifest.launch_arguments,
            working_directory,
            executables,
            environment: manifest.environment,
        },
    })
}

pub fn preflight_declaration(
    path: &Path,
    manifest: ManifestDeclaration,
) -> Result<UploadDeclaration, PublisherError> {
    let inspection = inspect_archive(path)?;
    build_declaration(&inspection, manifest.into())
}

#[tauri::command]
pub(crate) async fn inspect_publish_archive(
    archive_path: String,
) -> Result<PublishArchiveInspection, PublisherError> {
    let path = PathBuf::from(archive_path);
    tokio::task::spawn_blocking(move || inspect_archive(&path)?.into_public())
        .await
        .map_err(|_| PublisherError::unavailable("archive inspection task failed"))?
}

#[derive(Debug, Clone, Deserialize)]
struct UploadAuthorization {
    artifact: PublisherArtifact,
    upload: SignedUpload,
}

#[derive(Debug, Clone, Deserialize)]
struct SignedUpload {
    url: String,
    expires_at: String,
    required_headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Serialize)]
#[serde(rename_all(serialize = "camelCase", deserialize = "snake_case"))]
pub struct PublisherArtifact {
    id: String,
    release_id: String,
    platform: String,
    architecture: String,
    archive_format: String,
    compressed_size_bytes: Option<String>,
    installed_size_bytes: Option<String>,
    sha256: Option<String>,
    status: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ConfirmationApi {
    artifact: PublisherArtifact,
    release: PublisherReleaseApi,
    published: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishConfirmation {
    artifact: PublisherArtifact,
    release: PublisherRelease,
    published: bool,
}

impl From<ConfirmationApi> for PublishConfirmation {
    fn from(value: ConfirmationApi) -> Self {
        Self {
            artifact: value.artifact,
            release: value.release.into(),
            published: value.published,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublisherProgress {
    release_id: String,
    phase: String,
    uploaded_bytes: u64,
    total_bytes: u64,
    attempt: u32,
}

fn emit_progress<R: Runtime>(
    app: &AppHandle<R>,
    release_id: &str,
    phase: &str,
    uploaded_bytes: u64,
    total_bytes: u64,
    attempt: u32,
) {
    let _ = app.emit(
        "publisher-progress",
        PublisherProgress {
            release_id: release_id.into(),
            phase: phase.into(),
            uploaded_bytes,
            total_bytes,
            attempt,
        },
    );
}

#[derive(Default)]
pub struct PublishUploadManager {
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl PublishUploadManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn begin(&self, release_id: &str) -> Result<Arc<AtomicBool>, PublisherError> {
        let mut cancellations = self
            .cancellations
            .lock()
            .map_err(|_| PublisherError::unavailable("publisher manager is unavailable"))?;
        if cancellations.contains_key(release_id) {
            return Err(PublisherError::invalid(
                "an upload is already running for this release",
            ));
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        cancellations.insert(release_id.into(), cancellation.clone());
        Ok(cancellation)
    }

    fn finish(&self, release_id: &str) {
        if let Ok(mut cancellations) = self.cancellations.lock() {
            cancellations.remove(release_id);
        }
    }

    fn cancel(&self, release_id: &str) -> Result<(), PublisherError> {
        let cancellations = self
            .cancellations
            .lock()
            .map_err(|_| PublisherError::unavailable("publisher manager is unavailable"))?;
        let cancellation = cancellations.get(release_id).ok_or_else(|| {
            PublisherError::invalid("no active upload was found for this release")
        })?;
        cancellation.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn cancel_publish_upload(
    manager: tauri::State<'_, PublishUploadManager>,
    release_id: String,
) -> Result<(), PublisherError> {
    manager.cancel(&release_id)
}

fn authorization_expires_soon(expires_at: &str) -> bool {
    OffsetDateTime::parse(expires_at, &Rfc3339)
        .map(|expires| {
            expires - OffsetDateTime::now_utc()
                <= Duration::seconds(AUTHORIZATION_SAFETY_WINDOW_SECONDS)
        })
        .unwrap_or(true)
}

fn upload_url_allowed(url: &url::Url, development: bool) -> bool {
    if url.scheme() == "https" {
        return true;
    }
    development
        && url.scheme() == "http"
        && url.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
}

fn required_header_map(
    required: &BTreeMap<String, String>,
    total_bytes: u64,
) -> Result<HeaderMap, PublisherError> {
    let mut headers = HeaderMap::new();
    for (name, value) in required {
        let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
            PublisherError::invalid("upload authorization contains an invalid header")
        })?;
        if matches!(name, AUTHORIZATION | COOKIE | PROXY_AUTHORIZATION | HOST) {
            return Err(PublisherError::invalid(
                "upload authorization contains a forbidden header",
            ));
        }
        let value = HeaderValue::from_str(value).map_err(|_| {
            PublisherError::invalid("upload authorization contains an invalid header")
        })?;
        headers.insert(name, value);
    }
    let expected_length = total_bytes.to_string();
    if let Some(declared) = headers.get(CONTENT_LENGTH) {
        if declared.to_str().ok() != Some(expected_length.as_str()) {
            return Err(PublisherError::invalid(
                "upload authorization content length does not match the archive",
            ));
        }
    } else {
        headers.insert(
            CONTENT_LENGTH,
            HeaderValue::from_str(&expected_length)
                .map_err(|_| PublisherError::invalid("invalid archive size"))?,
        );
    }
    if !headers.contains_key(CONTENT_TYPE) {
        return Err(PublisherError::invalid(
            "upload authorization is missing content-type",
        ));
    }
    Ok(headers)
}

async fn request_upload_authorization(
    client: &Client,
    release_id: &str,
    declaration: &UploadDeclaration,
) -> Result<UploadAuthorization, PublisherError> {
    api_post(
        client,
        &format!("releases/{release_id}/artifacts/upload-url"),
        declaration,
    )
    .await
}

async fn upload_once<R: Runtime>(
    app: &AppHandle<R>,
    release_id: &str,
    archive_path: &Path,
    total_bytes: u64,
    authorization: &UploadAuthorization,
    cancellation: Arc<AtomicBool>,
    attempt: u32,
) -> Result<StatusCode, PublisherError> {
    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    let url = url::Url::parse(&authorization.upload.url)
        .map_err(|_| PublisherError::invalid("upload authorization contains an invalid URL"))?;
    if !upload_url_allowed(&url, publisher_environment() == "development") {
        return Err(PublisherError::invalid(
            "artifact uploads must use HTTPS outside local development",
        ));
    }
    let headers = required_header_map(&authorization.upload.required_headers, total_bytes)?;
    let file = File::open(archive_path)
        .await
        .map_err(|_| PublisherError::invalid("could not open archive for upload"))?;
    let app_for_stream = app.clone();
    let release_for_stream = release_id.to_string();
    let cancellation_for_stream = cancellation.clone();
    let stream = stream::try_unfold(
        (BufReader::new(file), 0_u64),
        move |(mut reader, uploaded)| {
            let app = app_for_stream.clone();
            let release_id = release_for_stream.clone();
            let cancellation = cancellation_for_stream.clone();
            async move {
                if cancellation.load(Ordering::Relaxed) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Interrupted,
                        "upload cancelled",
                    ));
                }
                let mut buffer = vec![0_u8; UPLOAD_BUFFER_SIZE];
                let count = reader.read(&mut buffer).await?;
                if count == 0 {
                    return Ok(None);
                }
                buffer.truncate(count);
                let uploaded = uploaded + count as u64;
                emit_progress(
                    &app,
                    &release_id,
                    "uploading",
                    uploaded,
                    total_bytes,
                    attempt,
                );
                Ok(Some((buffer, (reader, uploaded))))
            }
        },
    );

    let upload_client = Client::builder()
        .build()
        .map_err(|_| PublisherError::unavailable("could not initialize uploader"))?;
    let result = upload_client
        .put(url)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    result
        .map(|response| response.status())
        .map_err(|_| PublisherError::new("UPLOAD_FAILED", "artifact upload failed", true))
}

async fn publish_release_inner<R: Runtime>(
    app: &AppHandle<R>,
    api_client: &Client,
    release_id: &str,
    archive_path: PathBuf,
    manifest: PublishManifest,
    cancellation: Arc<AtomicBool>,
) -> Result<PublishConfirmation, PublisherError> {
    emit_progress(app, release_id, "analyzing", 0, 0, 0);
    let inspection = tokio::task::spawn_blocking(move || inspect_archive(&archive_path))
        .await
        .map_err(|_| PublisherError::unavailable("archive inspection task failed"))??;
    let total_bytes = inspection.compressed_size_bytes;
    let declaration = build_declaration(&inspection, manifest)?;

    let mut refreshes = 0_u32;
    let mut attempt = 1_u32;
    let mut authorization =
        request_upload_authorization(api_client, release_id, &declaration).await?;
    let artifact_id = authorization.artifact.id.clone();

    loop {
        if cancellation.load(Ordering::Relaxed) {
            return Err(PublisherError::cancelled());
        }
        while authorization_expires_soon(&authorization.upload.expires_at) {
            if refreshes >= MAX_AUTHORIZATION_REFRESHES {
                return Err(PublisherError::new(
                    "UPLOAD_AUTHORIZATION_EXPIRED",
                    "upload authorization expired before transfer",
                    true,
                ));
            }
            refreshes += 1;
            authorization =
                request_upload_authorization(api_client, release_id, &declaration).await?;
            if authorization.artifact.id != artifact_id {
                return Err(PublisherError::unavailable(
                    "artifact identity changed while refreshing upload authorization",
                ));
            }
        }

        emit_progress(app, release_id, "uploading", 0, total_bytes, attempt);
        let status = upload_once(
            app,
            release_id,
            &inspection.archive_path,
            total_bytes,
            &authorization,
            cancellation.clone(),
            attempt,
        )
        .await?;
        if status.is_success() {
            break;
        }
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
            && refreshes < MAX_AUTHORIZATION_REFRESHES
        {
            refreshes += 1;
            attempt += 1;
            authorization =
                request_upload_authorization(api_client, release_id, &declaration).await?;
            if authorization.artifact.id != artifact_id {
                return Err(PublisherError::unavailable(
                    "artifact identity changed while refreshing upload authorization",
                ));
            }
            continue;
        }
        return Err(PublisherError::new(
            if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
                "UPLOAD_AUTHORIZATION_EXPIRED"
            } else {
                "UPLOAD_FAILED"
            },
            format!("artifact storage returned {status}"),
            true,
        ));
    }

    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    emit_progress(
        app,
        release_id,
        "verifying",
        total_bytes,
        total_bytes,
        attempt,
    );
    let confirmation: ConfirmationApi =
        api_post_empty(api_client, &format!("artifacts/{artifact_id}/confirm")).await?;
    let confirmation: PublishConfirmation = confirmation.into();
    if confirmation.artifact.id != artifact_id
        || confirmation.release.id != release_id
        || confirmation.artifact.status != "READY"
        || confirmation.release.status != "PUBLISHED"
    {
        return Err(PublisherError::unavailable(
            "Manifold API returned an inconsistent publication result",
        ));
    }
    emit_progress(
        app,
        release_id,
        "published",
        total_bytes,
        total_bytes,
        attempt,
    );
    Ok(confirmation)
}

#[tauri::command]
pub(crate) async fn publish_release(
    app: AppHandle,
    state: tauri::State<'_, ApiState>,
    manager: tauri::State<'_, PublishUploadManager>,
    release_id: String,
    archive_path: String,
    manifest: PublishManifest,
) -> Result<PublishConfirmation, PublisherError> {
    if release_id.is_empty()
        || release_id.len() > 128
        || !release_id
            .chars()
            .all(|character| character.is_ascii_hexdigit() || character == '-')
    {
        return Err(PublisherError::invalid("invalid release id"));
    }
    let cancellation = manager.begin(&release_id)?;
    let api_client = state.client().map_err(PublisherError::unavailable)?;
    let result = publish_release_inner(
        &app,
        &api_client,
        &release_id,
        PathBuf::from(archive_path),
        manifest,
        cancellation,
    )
    .await;
    manager.finish(&release_id);
    if let Err(error) = &result {
        emit_progress(
            &app,
            &release_id,
            if error.code == "UPLOAD_CANCELLED" {
                "cancelled"
            } else {
                "failed"
            },
            0,
            0,
            0,
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = SyncFile::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        for (name, contents) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(contents).unwrap();
        }
        archive.finish().unwrap();
    }

    fn manifest(entrypoint: &str) -> PublishManifest {
        PublishManifest {
            schema_version: "1".into(),
            entrypoint: entrypoint.into(),
            launch_arguments: Vec::new(),
            working_directory: parent_path(entrypoint),
            executables: Vec::new(),
            environment: BTreeMap::new(),
        }
    }

    #[test]
    fn inspects_and_discovers_multiple_executables_in_stable_order() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(
            &path,
            &[
                ("Game/UnityCrashHandler64.exe", b"crash"),
                ("Game/Game.exe", b"game"),
                ("Game/data.bin", b"data"),
            ],
        );

        let inspection = inspect_archive(&path).unwrap().into_public().unwrap();
        assert_eq!(
            inspection.executables,
            vec!["Game/Game.exe", "Game/UnityCrashHandler64.exe"]
        );
        assert_eq!(inspection.suggested_entrypoint, "Game/Game.exe");
        assert_eq!(
            inspection.suggested_working_directory.as_deref(),
            Some("Game")
        );
        assert_eq!(inspection.sha256.len(), 64);
    }

    #[test]
    fn declaration_matches_the_backend_upload_contract() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(&path, &[("bin/game.exe", b"game"), ("data.bin", b"data")]);
        let inspection = inspect_archive(&path).unwrap();
        let declaration = build_declaration(&inspection, manifest("bin/game.exe")).unwrap();
        let json = serde_json::to_value(declaration).unwrap();

        assert_eq!(json["platform"], "WINDOWS");
        assert_eq!(json["architecture"], "X86_64");
        assert_eq!(json["archive_format"], "ZIP");
        assert_eq!(json["manifest"]["schema_version"], "1");
        assert_eq!(json["manifest"]["entrypoint"], "bin/game.exe");
        assert!(json["manifest"].get("release_id").is_none());
        assert!(json["manifest"].get("artifact_id").is_none());
    }

    #[test]
    fn rejects_invalid_zip_traversal_duplicate_windows_paths_and_no_exe() {
        let directory = tempfile::tempdir().unwrap();
        let invalid = directory.path().join("invalid.zip");
        std::fs::write(&invalid, b"not-a-zip").unwrap();
        assert!(inspect_archive(&invalid).is_err());

        let traversal = directory.path().join("traversal.zip");
        write_zip(&traversal, &[("../escape.exe", b"bad")]);
        assert!(inspect_archive(&traversal).is_err());

        let duplicate = directory.path().join("duplicate.zip");
        write_zip(
            &duplicate,
            &[("Game/Game.exe", b"one"), ("game/game.exe", b"two")],
        );
        assert!(inspect_archive(&duplicate).is_err());

        let no_exe = directory.path().join("no-exe.zip");
        write_zip(&no_exe, &[("readme.txt", b"text")]);
        assert!(inspect_archive(&no_exe).is_err());
    }

    #[test]
    fn rejects_symlinks_and_extreme_compression_ratios() {
        let directory = tempfile::tempdir().unwrap();
        let symlink = directory.path().join("symlink.zip");
        let file = SyncFile::create(&symlink).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .add_symlink(
                "game.exe",
                "target",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        archive.finish().unwrap();
        assert!(inspect_archive(&symlink).is_err());

        let bomb = directory.path().join("bomb.zip");
        let file = SyncFile::create(&bomb).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(
                "game.exe",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
        archive.write_all(&vec![0_u8; 2 * 1024 * 1024]).unwrap();
        archive.finish().unwrap();
        assert!(inspect_archive(&bomb).is_err());
    }

    #[test]
    fn validates_manifest_paths_against_the_inspected_archive() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game.zip");
        write_zip(&path, &[("Game/Game.exe", b"game")]);
        let inspection = inspect_archive(&path).unwrap();

        assert!(build_declaration(&inspection, manifest("../Game.exe")).is_err());
        assert!(build_declaration(&inspection, manifest("missing.exe")).is_err());
        assert!(build_declaration(&inspection, manifest("game/game.exe")).is_ok());
    }

    #[test]
    fn confirmation_accepts_backend_snake_case_and_returns_frontend_camel_case() {
        let api: ConfirmationApi = serde_json::from_value(serde_json::json!({
            "artifact": {
                "id": "artifact-1",
                "release_id": "release-1",
                "platform": "WINDOWS",
                "architecture": "X86_64",
                "archive_format": "ZIP",
                "compressed_size_bytes": "10",
                "installed_size_bytes": "20",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "status": "READY"
            },
            "release": {
                "id": "release-1",
                "game_id": "game-1",
                "version": "1.0.0",
                "release_number": 1,
                "status": "PUBLISHED",
                "published_at": "2026-08-26T12:00:00.000Z",
                "created_at": "2026-08-26T11:00:00.000Z",
                "updated_at": "2026-08-26T12:00:00.000Z"
            },
            "published": true
        }))
        .unwrap();
        let value = serde_json::to_value(PublishConfirmation::from(api)).unwrap();
        assert_eq!(value["artifact"]["releaseId"], "release-1");
        assert_eq!(value["release"]["releaseNotes"], serde_json::Value::Null);
        assert!(value["artifact"].get("release_id").is_none());
    }

    #[test]
    fn maps_authentication_and_permission_errors_structurally() {
        assert_eq!(
            error_for_status(StatusCode::UNAUTHORIZED, "sign in").code,
            "AUTHENTICATION_REQUIRED"
        );
        assert_eq!(
            error_for_status(StatusCode::FORBIDDEN, "not allowed").code,
            "PERMISSION_DENIED"
        );
        assert!(!error_for_status(StatusCode::FORBIDDEN, "not allowed").retryable);
    }

    #[test]
    fn rejects_sensitive_or_inconsistent_signed_upload_headers() {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/zip".into());
        headers.insert("cookie".into(), "secret".into());
        assert!(required_header_map(&headers, 10).is_err());

        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/zip".into());
        headers.insert("content-length".into(), "11".into());
        assert!(required_header_map(&headers, 10).is_err());
    }

    #[test]
    fn cancellation_manager_is_scoped_by_release() {
        let manager = PublishUploadManager::new();
        let cancellation = manager.begin("release-1").unwrap();
        assert!(manager.begin("release-1").is_err());
        manager.cancel("release-1").unwrap();
        assert!(cancellation.load(Ordering::Relaxed));
        manager.finish("release-1");
        assert!(manager.cancel("release-1").is_err());
    }

    #[test]
    fn expired_or_invalid_authorizations_refresh_before_upload() {
        assert!(authorization_expires_soon("invalid"));
        assert!(authorization_expires_soon("2000-01-01T00:00:00Z"));
        assert!(!authorization_expires_soon("2999-01-01T00:00:00Z"));
    }

    #[test]
    fn only_allows_insecure_uploads_to_loopback_development() {
        assert!(upload_url_allowed(
            &url::Url::parse("http://127.0.0.1:9000/file").unwrap(),
            true,
        ));
        assert!(!upload_url_allowed(
            &url::Url::parse("http://storage.example.test/file").unwrap(),
            true,
        ));
        assert!(upload_url_allowed(
            &url::Url::parse("https://storage.example.test/file").unwrap(),
            false,
        ));
    }
}
