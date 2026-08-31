use super::*;
use crate::butler::{Butler, WHARF_ALGORITHM, WHARF_FORMAT_VERSION};
use futures_util::StreamExt;
use serde_json::from_slice;
use sha2::{Digest, Sha256};
use std::{fs, fs::File as StdFile, io::Write as _, time::Instant};
use tauri::{Emitter, Manager};
use tokio::io::AsyncWriteExt;

const PATCH_SIZE_PERCENT_LIMIT: u64 = 80;
const PATCH_RECOVERY_SCHEMA_VERSION: u8 = 1;

pub(super) struct PatchPreparation<'a> {
    pub game_slug: &'a str,
    pub target_release_id: &'a str,
    pub platform: &'a str,
    pub architecture: &'a str,
    pub target_archive: &'a Path,
    pub target_compressed_size: u64,
    pub target_installed_size: u64,
    pub target_sha256: &'a str,
}

struct RecoveryExpectation<'a> {
    target_release_id: &'a str,
    source_release_id: &'a str,
    source_artifact_id: &'a str,
    source_sha256: &'a str,
    target_sha256: &'a str,
    old_archive: &'a Path,
    target_archive: &'a Path,
}

#[derive(Debug, Deserialize)]
struct SourceDownloadAuthorization {
    artifact_id: String,
    url: String,
    total_size_bytes: String,
    sha256: String,
    etag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
struct FileDeclaration {
    size_bytes: String,
    sha256: String,
}

#[derive(Debug, Serialize)]
struct PatchUploadRequest<'a> {
    source_release_id: &'a str,
    platform: &'a str,
    architecture: &'a str,
    algorithm: &'static str,
    format_version: &'static str,
    patch: FileDeclaration,
    signature: FileDeclaration,
    expected_installation_sha256: String,
    generation_duration_ms: String,
}

#[derive(Debug, Deserialize)]
struct PatchUploadResponse {
    patch: ConfirmedPatch,
    uploads: Option<PatchUploads>,
}

#[derive(Debug, Deserialize)]
struct PatchUploads {
    patch: SignedUpload,
    signature: SignedUpload,
}

#[derive(Debug, Deserialize)]
struct ConfirmedPatch {
    id: String,
    target_release_id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct ConfirmPatchResponse {
    patch: ConfirmedPatch,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum PatchRecoveryPhase {
    Generated,
    Validated,
    Uploaded,
    Confirmed,
    SkippedSizeLimit,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PatchRecovery {
    schema_version: u8,
    release_id: String,
    source_release_id: String,
    source_artifact_id: String,
    source_archive_path: String,
    target_archive_path: String,
    patch_path: String,
    signature_path: String,
    source_sha256: String,
    target_sha256: String,
    patch: FileDeclaration,
    signature: FileDeclaration,
    generation_duration_ms: String,
    temporary_bytes_required: String,
    phase: PatchRecoveryPhase,
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

fn select_source_release(
    releases: &[PublisherReleaseApi],
    target_release_id: &str,
) -> Result<Option<PublisherReleaseApi>, PublisherError> {
    let target_release_number = releases
        .iter()
        .find(|release| release.id == target_release_id)
        .ok_or_else(|| PublisherError::invalid("target release was not found in the game"))?
        .release_number;
    if target_release_number == 1 {
        return Ok(None);
    }
    releases
        .iter()
        .find(|release| {
            release.release_number + 1 == target_release_number && release.status == "PUBLISHED"
        })
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            PublisherError::new(
                "PATCH_SOURCE_UNAVAILABLE",
                "the immediately preceding published release is unavailable",
                true,
            )
        })
}

async fn source_release(
    client: &Client,
    game_slug: &str,
    target_release_id: &str,
    platform: &str,
    architecture: &str,
) -> Result<Option<(PublisherReleaseApi, PublisherArtifactApi)>, PublisherError> {
    let envelope: PublisherReleaseListApi = api_get(
        client,
        &format!("games/{game_slug}/releases?page=1&limit=100"),
    )
    .await?;
    let Some(source) = select_source_release(&envelope.releases, target_release_id)? else {
        return Ok(None);
    };
    let artifact = source
        .artifacts
        .iter()
        .find(|artifact| {
            artifact.platform == platform
                && artifact.architecture == architecture
                && artifact.archive_format == "ZIP"
                && artifact.status == "READY"
        })
        .cloned()
        .ok_or_else(|| {
            PublisherError::new(
                "PATCH_SOURCE_UNAVAILABLE",
                "the preceding target artifact is unavailable",
                true,
            )
        })?;
    if artifact
        .sha256
        .as_deref()
        .is_none_or(|sha| !valid_sha256(sha))
    {
        return Err(PublisherError::new(
            "PATCH_SOURCE_UNAVAILABLE",
            "the preceding artifact has no trusted SHA-256",
            true,
        ));
    }
    Ok(Some((source, artifact)))
}

async fn download_source<R: Runtime>(
    app: &AppHandle<R>,
    client: &Client,
    artifact: &PublisherArtifactApi,
    cancellation: &AtomicBool,
) -> Result<PathBuf, PublisherError> {
    let expected = artifact
        .sha256
        .as_deref()
        .ok_or_else(|| PublisherError::unavailable("source checksum is unavailable"))?;
    let root = app
        .path()
        .app_data_dir()
        .map_err(|_| PublisherError::unavailable("could not resolve publisher cache"))?
        .join("publisher-cache");
    tokio::fs::create_dir_all(&root)
        .await
        .map_err(|_| PublisherError::unavailable("could not create publisher cache"))?;
    let cache = root.join(format!("{}.zip", artifact.id));
    if cache.is_file() && sha256_file(&cache)? == expected {
        return Ok(cache);
    }
    if cache.exists() {
        tokio::fs::remove_file(&cache).await.map_err(|_| {
            PublisherError::unavailable("could not replace invalid publisher cache")
        })?;
    }
    let authorization: SourceDownloadAuthorization =
        api_post_empty(client, &format!("artifacts/{}/download", artifact.id)).await?;
    if authorization.artifact_id != artifact.id || authorization.sha256 != expected {
        return Err(PublisherError::unavailable(
            "source download authorization changed artifact identity",
        ));
    }
    let total = authorization
        .total_size_bytes
        .parse::<u64>()
        .map_err(|_| PublisherError::unavailable("source download size is invalid"))?;
    let url = url::Url::parse(&authorization.url)
        .map_err(|_| PublisherError::unavailable("source download URL is invalid"))?;
    if url.scheme() != "https"
        && !(publisher_environment() == "development"
            && url
                .host_str()
                .is_some_and(|host| matches!(host, "localhost" | "127.0.0.1" | "::1")))
    {
        return Err(PublisherError::unavailable(
            "source downloads must use HTTPS",
        ));
    }
    let response = Client::new()
        .get(url)
        .send()
        .await
        .map_err(|_| PublisherError::unavailable("could not download preceding release"))?;
    if !response.status().is_success() {
        return Err(error_for_status(
            response.status(),
            "preceding release download failed",
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length != total)
    {
        return Err(PublisherError::unavailable(
            "preceding release size changed",
        ));
    }
    if let Some(expected_etag) = authorization.etag.as_deref() {
        if response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value != expected_etag)
        {
            return Err(PublisherError::unavailable(
                "preceding release ETag changed",
            ));
        }
    }
    let temporary = cache.with_extension("zip.part");
    let mut file = tokio::fs::File::create(&temporary)
        .await
        .map_err(|_| PublisherError::unavailable("could not create publisher cache file"))?;
    let mut received = 0_u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancellation.load(Ordering::Relaxed) {
            let _ = tokio::fs::remove_file(&temporary).await;
            return Err(PublisherError::cancelled());
        }
        let chunk = chunk.map_err(|_| {
            PublisherError::unavailable("preceding release download was interrupted")
        })?;
        received = received.saturating_add(chunk.len() as u64);
        if received > total {
            return Err(PublisherError::unavailable(
                "preceding release exceeded declared size",
            ));
        }
        file.write_all(&chunk)
            .await
            .map_err(|_| PublisherError::unavailable("could not save preceding release"))?;
    }
    file.flush()
        .await
        .map_err(|_| PublisherError::unavailable("could not flush publisher cache"))?;
    drop(file);
    if received != total || sha256_file(&temporary)? != expected {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(PublisherError::unavailable(
            "preceding release failed SHA-256 verification",
        ));
    }
    tokio::fs::rename(&temporary, &cache)
        .await
        .map_err(|_| PublisherError::unavailable("could not finalize publisher cache"))?;
    Ok(cache)
}

fn sha256_file(path: &Path) -> Result<String, PublisherError> {
    let mut file = StdFile::open(path)
        .map_err(|_| PublisherError::unavailable("could not open generated patch file"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| PublisherError::unavailable("could not hash generated patch file"))?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn write_recovery(path: &Path, recovery: &PatchRecovery) -> Result<(), PublisherError> {
    let bytes = serde_json::to_vec_pretty(recovery)
        .map_err(|_| PublisherError::unavailable("could not serialize publisher recovery"))?;
    let temporary = path.with_extension("tmp");
    let backup = path.with_extension("bak");
    let mut file = StdFile::create(&temporary)
        .map_err(|_| PublisherError::unavailable("could not create publisher recovery"))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| PublisherError::unavailable("could not persist publisher recovery"))?;
    drop(file);

    if backup.exists() {
        fs::remove_file(&backup)
            .map_err(|_| PublisherError::unavailable("could not rotate publisher recovery"))?;
    }
    let had_previous = path.exists();
    if had_previous {
        fs::rename(path, &backup)
            .map_err(|_| PublisherError::unavailable("could not rotate publisher recovery"))?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        if had_previous {
            let _ = fs::rename(&backup, path);
        }
        return Err(PublisherError::unavailable(format!(
            "could not finalize publisher recovery: {error}"
        )));
    }
    if had_previous {
        let _ = fs::remove_file(backup);
    }
    Ok(())
}

fn read_recovery(path: &Path) -> Option<PatchRecovery> {
    let backup = path.with_extension("bak");
    fs::read(path)
        .ok()
        .or_else(|| fs::read(backup).ok())
        .and_then(|bytes| from_slice(&bytes).ok())
}

fn ready_patch_can_skip_upload(status: &str, uploads_present: bool) -> bool {
    status == "READY" && !uploads_present
}

fn patch_within_limit(patch_size: u64, full_size: u64) -> bool {
    patch_size.saturating_mul(100) <= full_size.saturating_mul(PATCH_SIZE_PERCENT_LIMIT)
}

fn required_temporary_space(
    source_compressed: u64,
    source_installed: u64,
    target_compressed: u64,
    target_installed: u64,
) -> u64 {
    source_compressed
        .saturating_add(source_installed)
        .saturating_add(target_compressed)
        .saturating_add(target_installed.saturating_mul(2))
}

fn emit_patch_progress<R: Runtime>(
    app: &AppHandle<R>,
    release_id: &str,
    phase: &str,
    uploaded_bytes: u64,
    total_bytes: u64,
    temporary_bytes_required: Option<u64>,
) {
    let _ = app.emit(
        "publisher-progress",
        PublisherProgress {
            release_id: release_id.into(),
            phase: phase.into(),
            uploaded_bytes,
            total_bytes,
            attempt: 1,
            temporary_bytes_required,
        },
    );
}

async fn upload_signed_file<R: Runtime>(
    app: &AppHandle<R>,
    release_id: &str,
    path: &Path,
    authorization: &SignedUpload,
    cancellation: Arc<AtomicBool>,
    offset: u64,
    combined_total: u64,
) -> Result<(), PublisherError> {
    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    let size = fs::metadata(path)
        .map_err(|_| PublisherError::unavailable("generated patch file disappeared"))?
        .len();
    let url = url::Url::parse(&authorization.url)
        .map_err(|_| PublisherError::unavailable("patch upload URL is invalid"))?;
    if !upload_url_allowed(&url, publisher_environment() == "development") {
        return Err(PublisherError::unavailable("patch uploads must use HTTPS"));
    }
    let headers = required_header_map(&authorization.required_headers, size)?;
    let file = File::open(path)
        .await
        .map_err(|_| PublisherError::unavailable("could not open generated patch file"))?;
    let release_for_stream = release_id.to_string();
    let app_for_stream = app.clone();
    let cancellation_for_stream = cancellation.clone();
    let stream = stream::try_unfold(
        (BufReader::new(file), 0_u64),
        move |(mut reader, uploaded)| {
            let cancellation = cancellation_for_stream.clone();
            let release_id = release_for_stream.clone();
            let app = app_for_stream.clone();
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
                let uploaded = uploaded.saturating_add(count as u64);
                emit_patch_progress(
                    &app,
                    &release_id,
                    "uploading_patch",
                    offset.saturating_add(uploaded),
                    combined_total,
                    None,
                );
                Ok(Some((buffer, (reader, uploaded))))
            }
        },
    );
    let result = Client::new()
        .put(url)
        .headers(headers)
        .body(reqwest::Body::wrap_stream(stream))
        .send()
        .await;
    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    let response = result
        .map_err(|_| PublisherError::new("PATCH_UPLOAD_FAILED", "patch upload failed", true))?;
    if !response.status().is_success() {
        return Err(error_for_status(
            response.status(),
            "patch storage rejected the upload",
        ));
    }
    Ok(())
}

fn recovery_matches(recovery: &PatchRecovery, expected: &RecoveryExpectation<'_>) -> bool {
    recovery.schema_version == PATCH_RECOVERY_SCHEMA_VERSION
        && recovery.release_id == expected.target_release_id
        && recovery.source_release_id == expected.source_release_id
        && recovery.source_artifact_id == expected.source_artifact_id
        && recovery.source_sha256 == expected.source_sha256
        && recovery.target_sha256 == expected.target_sha256
        && recovery.source_archive_path == expected.old_archive.to_string_lossy()
        && recovery.target_archive_path == expected.target_archive.to_string_lossy()
}

pub(super) async fn prepare_and_confirm_patch<R: Runtime>(
    app: &AppHandle<R>,
    client: &Client,
    preparation: PatchPreparation<'_>,
    cancellation: Arc<AtomicBool>,
) -> Result<(), PublisherError> {
    let PatchPreparation {
        game_slug,
        target_release_id,
        platform,
        architecture,
        target_archive,
        target_compressed_size,
        target_installed_size,
        target_sha256,
    } = preparation;
    let Some((source, artifact)) =
        source_release(client, game_slug, target_release_id, platform, architecture).await?
    else {
        return Ok(());
    };
    let old_archive = download_source(app, client, &artifact, &cancellation).await?;
    if cancellation.load(Ordering::Relaxed) {
        return Err(PublisherError::cancelled());
    }
    let source_sha256 = artifact
        .sha256
        .as_deref()
        .ok_or_else(|| PublisherError::unavailable("source SHA-256 is unavailable"))?;
    let source_compressed = artifact
        .compressed_size_bytes
        .as_deref()
        .ok_or_else(|| PublisherError::unavailable("source compressed size is unavailable"))?
        .parse::<u64>()
        .map_err(|_| PublisherError::unavailable("source compressed size is invalid"))?;
    let source_installed = artifact
        .installed_size_bytes
        .as_deref()
        .ok_or_else(|| PublisherError::unavailable("source installed size is unavailable"))?
        .parse::<u64>()
        .map_err(|_| PublisherError::unavailable("source installed size is invalid"))?;
    let work = app
        .path()
        .app_data_dir()
        .map_err(|_| PublisherError::unavailable("could not resolve publisher workspace"))?
        .join("publisher-patches")
        .join(target_release_id);
    fs::create_dir_all(&work)
        .map_err(|_| PublisherError::unavailable("could not create publisher patch workspace"))?;
    let recovery_path = work.join("recovery.json");
    let patch_path = work.join("update.pwr");
    let signature_path = work.join("update.pwr.sig");
    let rebuilt = work.join("rebuilt");
    let apply_stage = work.join("apply-stage");
    let required_space = required_temporary_space(
        source_compressed,
        source_installed,
        target_compressed_size,
        target_installed_size,
    );
    emit_patch_progress(
        app,
        target_release_id,
        "preparing_patch",
        0,
        target_compressed_size,
        Some(required_space),
    );
    let available_space = fs2::available_space(&work)
        .map_err(|_| PublisherError::unavailable("could not inspect temporary disk space"))?;
    if available_space < required_space {
        return Err(PublisherError::new(
            "PATCH_DISK_SPACE_INSUFFICIENT",
            format!(
                "incremental publishing needs {required_space} temporary bytes but only {available_space} are available"
            ),
            true,
        ));
    }

    let mut recovery = read_recovery(&recovery_path).filter(|value| {
        recovery_matches(
            value,
            &RecoveryExpectation {
                target_release_id,
                source_release_id: &source.id,
                source_artifact_id: &artifact.id,
                source_sha256,
                target_sha256,
                old_archive: &old_archive,
                target_archive,
            },
        )
    });
    if recovery.as_ref().is_some_and(|value| {
        matches!(
            value.phase,
            PatchRecoveryPhase::Confirmed | PatchRecoveryPhase::SkippedSizeLimit
        )
    }) {
        return Ok(());
    }
    if recovery.as_ref().is_some_and(|value| {
        !Path::new(&value.patch_path).is_file()
            || !Path::new(&value.signature_path).is_file()
            || sha256_file(Path::new(&value.patch_path)).ok().as_deref()
                != Some(value.patch.sha256.as_str())
            || sha256_file(Path::new(&value.signature_path))
                .ok()
                .as_deref()
                != Some(value.signature.sha256.as_str())
    }) {
        recovery = None;
    }

    if recovery.is_none() {
        for path in [&patch_path, &signature_path] {
            if path.exists() {
                fs::remove_file(path)
                    .map_err(|_| PublisherError::unavailable("could not reset generated patch"))?;
            }
        }
        for path in [&rebuilt, &apply_stage] {
            if path.exists() {
                fs::remove_dir_all(path).map_err(|_| {
                    PublisherError::unavailable("could not reset patch verification workspace")
                })?;
            }
        }
        let started = Instant::now();
        let butler = Butler::locate(app).map_err(PublisherError::unavailable)?;
        butler
            .diff(&old_archive, target_archive, &patch_path, &cancellation)
            .map_err(|error| PublisherError::new("PATCH_GENERATION_FAILED", error, true))?;
        if !signature_path.is_file() {
            return Err(PublisherError::new(
                "PATCH_GENERATION_FAILED",
                "Butler did not create the target signature",
                true,
            ));
        }
        let patch_size = fs::metadata(&patch_path)
            .map_err(|_| PublisherError::unavailable("generated patch disappeared"))?
            .len();
        let signature_size = fs::metadata(&signature_path)
            .map_err(|_| PublisherError::unavailable("generated signature disappeared"))?
            .len();
        let signature_sha = sha256_file(&signature_path)?;
        let mut generated = PatchRecovery {
            schema_version: PATCH_RECOVERY_SCHEMA_VERSION,
            release_id: target_release_id.into(),
            source_release_id: source.id.clone(),
            source_artifact_id: artifact.id.clone(),
            source_archive_path: old_archive.to_string_lossy().into_owned(),
            target_archive_path: target_archive.to_string_lossy().into_owned(),
            patch_path: patch_path.to_string_lossy().into_owned(),
            signature_path: signature_path.to_string_lossy().into_owned(),
            source_sha256: source_sha256.into(),
            target_sha256: target_sha256.into(),
            patch: FileDeclaration {
                size_bytes: patch_size.to_string(),
                sha256: sha256_file(&patch_path)?,
            },
            signature: FileDeclaration {
                size_bytes: signature_size.to_string(),
                sha256: signature_sha,
            },
            generation_duration_ms: started.elapsed().as_millis().max(1).to_string(),
            temporary_bytes_required: required_space.to_string(),
            phase: PatchRecoveryPhase::Generated,
        };
        if !patch_within_limit(patch_size, target_compressed_size) {
            generated.phase = PatchRecoveryPhase::SkippedSizeLimit;
            write_recovery(&recovery_path, &generated)?;
            return Ok(());
        }
        write_recovery(&recovery_path, &generated)?;
        recovery = Some(generated);
    }

    let mut recovery = recovery.ok_or_else(|| {
        PublisherError::unavailable("publisher recovery could not be initialized")
    })?;
    if recovery.phase == PatchRecoveryPhase::Generated {
        emit_patch_progress(
            app,
            target_release_id,
            "validating_patch",
            0,
            target_compressed_size,
            Some(required_space),
        );
        if rebuilt.exists() {
            fs::remove_dir_all(&rebuilt)
                .map_err(|_| PublisherError::unavailable("could not reset rebuilt target"))?;
        }
        if apply_stage.exists() {
            fs::remove_dir_all(&apply_stage)
                .map_err(|_| PublisherError::unavailable("could not reset Butler staging"))?;
        }
        fs::create_dir_all(&apply_stage)
            .map_err(|_| PublisherError::unavailable("could not create Butler staging"))?;
        let butler = Butler::locate(app).map_err(PublisherError::unavailable)?;
        butler
            .apply_to(
                &patch_path,
                &signature_path,
                &old_archive,
                &rebuilt,
                &apply_stage,
                &cancellation,
            )
            .map_err(|error| PublisherError::new("PATCH_VERIFICATION_FAILED", error, true))?;
        butler
            .verify(&signature_path, &rebuilt, &cancellation)
            .map_err(|error| PublisherError::new("PATCH_VERIFICATION_FAILED", error, true))?;
        recovery.phase = PatchRecoveryPhase::Validated;
        write_recovery(&recovery_path, &recovery)?;
    }

    let request = PatchUploadRequest {
        source_release_id: &recovery.source_release_id,
        platform,
        architecture,
        algorithm: WHARF_ALGORITHM,
        format_version: WHARF_FORMAT_VERSION,
        patch: recovery.patch.clone(),
        signature: recovery.signature.clone(),
        expected_installation_sha256: recovery.signature.sha256.clone(),
        generation_duration_ms: recovery.generation_duration_ms.clone(),
    };
    let initiated: PatchUploadResponse = api_post(
        client,
        &format!("releases/{target_release_id}/patches/upload-url"),
        &request,
    )
    .await?;
    if initiated.patch.target_release_id != target_release_id {
        return Err(PublisherError::unavailable(
            "patch service changed target identity",
        ));
    }
    if ready_patch_can_skip_upload(&initiated.patch.status, initiated.uploads.is_some()) {
        recovery.phase = PatchRecoveryPhase::Confirmed;
        write_recovery(&recovery_path, &recovery)?;
        return Ok(());
    }
    let uploads = initiated.uploads.ok_or_else(|| {
        PublisherError::new(
            "PATCH_UPLOAD_FAILED",
            "patch upload authorizations are unavailable",
            true,
        )
    })?;
    let patch_size = recovery
        .patch
        .size_bytes
        .parse::<u64>()
        .map_err(|_| PublisherError::unavailable("recovered patch size is invalid"))?;
    let signature_size = recovery
        .signature
        .size_bytes
        .parse::<u64>()
        .map_err(|_| PublisherError::unavailable("recovered signature size is invalid"))?;
    let combined = patch_size.saturating_add(signature_size);
    emit_patch_progress(app, target_release_id, "uploading_patch", 0, combined, None);
    upload_signed_file(
        app,
        target_release_id,
        &patch_path,
        &uploads.patch,
        cancellation.clone(),
        0,
        combined,
    )
    .await?;
    upload_signed_file(
        app,
        target_release_id,
        &signature_path,
        &uploads.signature,
        cancellation,
        patch_size,
        combined,
    )
    .await?;
    recovery.phase = PatchRecoveryPhase::Uploaded;
    write_recovery(&recovery_path, &recovery)?;
    let confirmed: ConfirmPatchResponse =
        api_post_empty(client, &format!("patches/{}/confirm", initiated.patch.id)).await?;
    if confirmed.patch.id != initiated.patch.id || confirmed.patch.status != "READY" {
        return Err(PublisherError::new(
            "PATCH_CONFIRMATION_FAILED",
            "patch confirmation did not become READY",
            true,
        ));
    }
    recovery.phase = PatchRecoveryPhase::Confirmed;
    write_recovery(&recovery_path, &recovery)?;
    Ok(())
}

pub(super) fn complete_publication<R: Runtime>(app: &AppHandle<R>, release_id: &str) {
    if let Ok(root) = app.path().app_data_dir() {
        let workspace = root.join("publisher-patches").join(release_id);
        if workspace.exists() {
            let _ = fs::remove_dir_all(workspace);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(id: &str, number: u64, status: &str) -> PublisherReleaseApi {
        PublisherReleaseApi {
            id: id.into(),
            game_id: "game".into(),
            version: number.to_string(),
            release_number: number,
            status: status.into(),
            release_notes: None,
            published_at: None,
            created_at: "2026-08-28T00:00:00Z".into(),
            updated_at: "2026-08-28T00:00:00Z".into(),
            artifacts: vec![],
        }
    }

    #[test]
    fn patch_threshold_is_inclusive_at_eighty_percent() {
        assert!(patch_within_limit(800, 1000));
        assert!(!patch_within_limit(801, 1000));
    }

    #[test]
    fn release_one_is_full_only() {
        assert!(
            select_source_release(&[release("target", 1, "DRAFT")], "target")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn missing_immediate_predecessor_fails_before_zip_publication() {
        let error = select_source_release(
            &[
                release("target", 3, "DRAFT"),
                release("old", 1, "PUBLISHED"),
            ],
            "target",
        )
        .unwrap_err();
        assert_eq!(error.code, "PATCH_SOURCE_UNAVAILABLE");
    }

    #[test]
    fn temporary_space_estimate_accounts_for_both_rebuilt_trees() {
        assert_eq!(required_temporary_space(10, 20, 30, 40), 140);
    }

    #[test]
    fn recovery_contains_no_signed_urls_or_credentials() {
        let recovery = PatchRecovery {
            schema_version: 1,
            release_id: "target".into(),
            source_release_id: "source".into(),
            source_artifact_id: "artifact".into(),
            source_archive_path: "source.zip".into(),
            target_archive_path: "target.zip".into(),
            patch_path: "update.pwr".into(),
            signature_path: "update.pwr.sig".into(),
            source_sha256: "a".repeat(64),
            target_sha256: "b".repeat(64),
            patch: FileDeclaration {
                size_bytes: "1".into(),
                sha256: "c".repeat(64),
            },
            signature: FileDeclaration {
                size_bytes: "1".into(),
                sha256: "d".repeat(64),
            },
            generation_duration_ms: "1".into(),
            temporary_bytes_required: "10".into(),
            phase: PatchRecoveryPhase::Validated,
        };
        let serialized = serde_json::to_string(&recovery).unwrap();
        assert!(!serialized.contains("url"));
        assert!(!serialized.contains("token"));
    }
    #[test]
    fn ready_retry_with_null_uploads_skips_upload_and_confirmation() {
        assert!(ready_patch_can_skip_upload("READY", false));
        assert!(!ready_patch_can_skip_upload("READY", true));
        assert!(!ready_patch_can_skip_upload("PENDING", false));
    }

    #[test]
    fn recovery_replaces_existing_file_and_reads_crash_backup() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("recovery.json");
        let mut recovery = PatchRecovery {
            schema_version: 1,
            release_id: "target".into(),
            source_release_id: "source".into(),
            source_artifact_id: "artifact".into(),
            source_archive_path: "source.zip".into(),
            target_archive_path: "target.zip".into(),
            patch_path: "update.pwr".into(),
            signature_path: "update.pwr.sig".into(),
            source_sha256: "a".repeat(64),
            target_sha256: "b".repeat(64),
            patch: FileDeclaration {
                size_bytes: "1".into(),
                sha256: "c".repeat(64),
            },
            signature: FileDeclaration {
                size_bytes: "1".into(),
                sha256: "d".repeat(64),
            },
            generation_duration_ms: "1".into(),
            temporary_bytes_required: "10".into(),
            phase: PatchRecoveryPhase::Generated,
        };
        write_recovery(&path, &recovery).unwrap();
        recovery.phase = PatchRecoveryPhase::Validated;
        write_recovery(&path, &recovery).unwrap();
        assert_eq!(
            read_recovery(&path).unwrap().phase,
            PatchRecoveryPhase::Validated
        );
        assert!(!path.with_extension("bak").exists());

        fs::rename(&path, path.with_extension("bak")).unwrap();
        assert_eq!(
            read_recovery(&path).unwrap().phase,
            PatchRecoveryPhase::Validated
        );
    }
}
