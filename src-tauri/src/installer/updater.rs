use super::*;
use crate::butler::{Butler, WHARF_ALGORITHM, WHARF_FORMAT_VERSION};

const PENDING_UPDATES_FILE: &str = "pending-updates.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateReleaseIdentity {
    pub id: String,
    pub version: String,
    pub release_number: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatchFileDeclaration {
    pub size_bytes: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReleasePatch {
    pub id: String,
    pub source_release_id: String,
    pub target_release_id: String,
    pub target: ReleaseTarget,
    pub algorithm: String,
    pub format_version: String,
    pub status: String,
    pub patch: PatchFileDeclaration,
    pub signature: PatchFileDeclaration,
    pub expected_installation_sha256: String,
    pub generation_duration_ms: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "strategy", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpdatePlan {
    Patch {
        source: UpdateReleaseIdentity,
        target: ReleaseSummary,
        fallback_artifact_id: String,
        patch: Box<ReleasePatch>,
    },
    Full {
        source: UpdateReleaseIdentity,
        target: ReleaseSummary,
        fallback_artifact_id: String,
        reason: String,
    },
}

impl UpdatePlan {
    fn source(&self) -> &UpdateReleaseIdentity {
        match self {
            Self::Patch { source, .. } | Self::Full { source, .. } => source,
        }
    }
    fn target(&self) -> &ReleaseSummary {
        match self {
            Self::Patch { target, .. } | Self::Full { target, .. } => target,
        }
    }
    fn fallback_artifact_id(&self) -> &str {
        match self {
            Self::Patch {
                fallback_artifact_id,
                ..
            }
            | Self::Full {
                fallback_artifact_id,
                ..
            } => fallback_artifact_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatchFileDownloadAuthorization {
    pub patch_id: String,
    pub file: String,
    pub url: String,
    pub expires_at: String,
    pub total_size_bytes: String,
    pub sha256: String,
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PatchDownloadAuthorizations {
    pub patch: PatchFileDownloadAuthorization,
    pub signature: PatchFileDownloadAuthorization,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct UpdateExecutionPlan {
    pub update: UpdatePlan,
    pub manifest: InstallManifest,
    pub patch_downloads: Option<PatchDownloadAuthorizations>,
    pub fallback_download: DownloadAuthorization,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum UpdateJournalPhase {
    Preparing,
    Downloading,
    Applying,
    Verifying,
    Activating,
    RegistryPersisted,
    FullFallback,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingUpdate {
    game_slug: String,
    source_release_id: String,
    target_release_id: String,
    phase: UpdateJournalPhase,
}

pub(super) fn pending_updates_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_directory(app)?.join(PENDING_UPDATES_FILE))
}

fn write_pending_update(path: &Path, operation: PendingUpdate) -> Result<(), String> {
    let mut pending: Vec<PendingUpdate> = read_json_or_default(path)?;
    pending.retain(|value| value.game_slug != operation.game_slug);
    pending.push(operation);
    write_json_atomic(path, &pending)
        .map_err(|error| format!("could not update pending update journal: {error}"))
}

fn clear_pending_update(path: &Path, game_slug: &str) -> Result<(), String> {
    let mut pending: Vec<PendingUpdate> = read_json_or_default(path)?;
    pending.retain(|value| value.game_slug != game_slug);
    write_json_atomic(path, &pending)
        .map_err(|error| format!("could not update pending update journal: {error}"))
}

fn journal(
    path: &Path,
    game_slug: &str,
    source: &str,
    target: &str,
    phase: UpdateJournalPhase,
) -> Result<(), String> {
    write_pending_update(
        path,
        PendingUpdate {
            game_slug: game_slug.into(),
            source_release_id: source.into(),
            target_release_id: target.into(),
            phase,
        },
    )
}

fn derived_update_paths(game: &InstalledGame) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let destination = PathBuf::from(&game.install_directory);
    let root = destination
        .parent()
        .ok_or("invalid managed installation path")?
        .to_path_buf();
    let expected = root.join(&game.game_slug);
    if !paths_match(&destination, &expected) {
        return Err("unsafe update path".into());
    }
    Ok((
        destination,
        root.join(format!(".{}.staging", game.game_slug)),
        root.join(format!(".{}.backup", game.game_slug)),
    ))
}

pub(super) fn recover_pending_updates_at(
    registry_file: &Path,
    pending_file: &Path,
) -> Result<(), String> {
    let pending: Vec<PendingUpdate> = read_json_or_default(pending_file)?;
    if pending.is_empty() {
        return Ok(());
    }
    let registry = read_registry_at(registry_file)?;
    for operation in &pending {
        let game = registry
            .iter()
            .find(|value| value.game_slug == operation.game_slug);
        let Some(game) = game else { continue };
        let (destination, stage, backup) = derived_update_paths(game)?;
        if matches!(
            operation.phase,
            UpdateJournalPhase::Activating | UpdateJournalPhase::RegistryPersisted
        ) {
            if game.release_id == operation.target_release_id {
                if backup.exists() {
                    finalize_activation_backup(&backup)?;
                }
            } else {
                rollback_activation(&destination, &backup);
            }
        }
        if stage.exists() {
            let _ = fs::remove_dir_all(&stage);
        }
    }
    write_json_atomic(pending_file, &Vec::<PendingUpdate>::new())
        .map_err(|error| format!("could not recover pending updates: {error}"))
}

fn validate_update(current: &InstalledGame, execution: &UpdateExecutionPlan) -> Result<(), String> {
    let update = &execution.update;
    let source = update.source();
    if current.release_id != source.id
        || current.version != source.version
        || current.release_number != source.release_number
        || update.target().release_number <= current.release_number
    {
        return Err("update source does not match the installed release".into());
    }

    let fallback = DistributionPlan {
        game_slug: current.game_slug.clone(),
        release: update.target().clone(),
        manifest: execution.manifest.clone(),
        download: execution.fallback_download.clone(),
    };
    if update.fallback_artifact_id() != update.target().artifact_id {
        return Err("update target declarations do not match".into());
    }
    validate_plan(&fallback).map_err(|_| "update target declarations do not match".to_string())?;

    match update {
        UpdatePlan::Patch { patch, .. } => {
            if update.target().release_number != current.release_number.saturating_add(1)
                || patch.source_release_id != current.release_id
                || patch.target_release_id != update.target().id
                || patch.target.platform != update.target().target.platform
                || patch.target.architecture != update.target().target.architecture
                || patch.algorithm != WHARF_ALGORITHM
                || patch.format_version != WHARF_FORMAT_VERSION
                || patch.status != "READY"
                || patch.expected_installation_sha256 != patch.signature.sha256
            {
                return Err("patch declaration is not applicable".into());
            }
            let downloads = execution
                .patch_downloads
                .as_ref()
                .ok_or("patch download authorizations are missing")?;
            if downloads.patch.patch_id != patch.id
                || downloads.signature.patch_id != patch.id
                || downloads.patch.file != "PATCH"
                || downloads.signature.file != "SIGNATURE"
                || downloads.patch.sha256 != patch.patch.sha256
                || downloads.signature.sha256 != patch.signature.sha256
                || downloads.patch.total_size_bytes != patch.patch.size_bytes
                || downloads.signature.total_size_bytes != patch.signature.size_bytes
            {
                return Err("patch download declarations do not match".into());
            }
            for authorization in [&downloads.patch, &downloads.signature] {
                let url = url::Url::parse(&authorization.url)
                    .map_err(|_| "invalid patch download URL")?;
                if url.scheme() != "https" {
                    return Err("patch downloads must use HTTPS".into());
                }
            }
        }
        UpdatePlan::Full { .. } if execution.patch_downloads.is_some() => {
            return Err("full update must not include patch downloads".into());
        }
        UpdatePlan::Full { .. } => {}
    }
    Ok(())
}
fn copy_tree(source: &Path, target: &Path, cancellation: &AtomicBool) -> Result<(), String> {
    ensure_not_cancelled(cancellation)?;
    fs::create_dir_all(target)
        .map_err(|error| format!("could not create update staging: {error}"))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("could not read active installation: {error}"))?
    {
        ensure_not_cancelled(cancellation)?;
        let entry =
            entry.map_err(|error| format!("could not inspect active installation: {error}"))?;
        let kind = entry
            .file_type()
            .map_err(|error| format!("could not inspect active installation: {error}"))?;
        let destination = target.join(entry.file_name());
        if kind.is_symlink() {
            return Err("active installation contains an unsafe symbolic link".into());
        }
        if kind.is_dir() {
            copy_tree(&entry.path(), &destination, cancellation)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), destination)
                .map_err(|error| format!("could not stage active installation: {error}"))?;
        } else {
            return Err("active installation contains an unsupported file".into());
        }
    }
    Ok(())
}

async fn download_update_file<F>(
    authorization: &PatchFileDownloadAuthorization,
    destination: &Path,
    cancellation: &AtomicBool,
    on_progress: F,
) -> Result<(), String>
where
    F: FnMut(u64),
{
    let total = authorization
        .total_size_bytes
        .parse::<u64>()
        .map_err(|_| "invalid patch size")?;
    let url = url::Url::parse(&authorization.url).map_err(|_| "invalid patch URL")?;
    if url.scheme() != "https" {
        return Err("patch downloads must use HTTPS".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| format!("could not initialize patch downloader: {error}"))?;
    download_to_file(
        &client,
        &authorization.url,
        destination,
        total,
        authorization.etag.as_deref(),
        cancellation,
        on_progress,
    )
    .await?;
    verify_archive_checksum(destination, &authorization.sha256)
}

fn patch_payload_paths(downloads_root: &Path, patch_id: &str) -> (PathBuf, PathBuf) {
    (
        downloads_root.join(format!("{patch_id}.pwr.part")),
        downloads_root.join(format!("{patch_id}.pwr.sig.part")),
    )
}

fn remove_patch_payload_files(patch_path: &Path, signature_path: &Path) {
    for path in [patch_path, signature_path] {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
    }
}

fn cleanup_patch_attempt(app: &AppHandle, current: &InstalledGame, patch_id: &str) {
    if let Ok(root) = app_data_directory(app) {
        let (patch_path, signature_path) = patch_payload_paths(&root.join("downloads"), patch_id);
        remove_patch_payload_files(&patch_path, &signature_path);
    }
    if let Ok((_, stage, _)) = derived_update_paths(current) {
        if let Some(parent) = stage.parent() {
            let butler_stage = parent.join(format!(".{}.butler", current.game_slug));
            if butler_stage.exists() {
                let _ = fs::remove_dir_all(butler_stage);
            }
        }
    }
}

fn build_installed(
    current: &InstalledGame,
    title: &str,
    execution: &UpdateExecutionPlan,
    destination: &Path,
) -> Result<InstalledGame, String> {
    let target = execution.update.target();
    Ok(InstalledGame {
        game_slug: current.game_slug.clone(),
        title: title.into(),
        version: target.version.clone(),
        release_id: target.id.clone(),
        release_number: target.release_number,
        artifact_id: target.artifact_id.clone(),
        installed_size_bytes: target.installed_size_bytes.clone(),
        install_directory: destination.to_string_lossy().into_owned(),
        entrypoint: execution.manifest.entrypoint.clone(),
        installed_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "system clock is invalid")?
            .as_secs()
            .to_string(),
        status: InstallationStatus::Installed,
        launch_arguments: execution.manifest.launch_arguments.clone(),
        working_directory: execution.manifest.working_directory.clone(),
        environment: execution.manifest.environment.clone(),
    })
}

struct UpdatePromotion<'a> {
    registry_file: &'a Path,
    pending_file: &'a Path,
    current: &'a InstalledGame,
    installed: &'a InstalledGame,
    stage: &'a Path,
    destination: &'a Path,
    backup: &'a Path,
}

fn promote_update(
    registry_file: &Path,
    pending_file: &Path,
    current: &InstalledGame,
    installed: &InstalledGame,
    stage: &Path,
    destination: &Path,
    backup: &Path,
) -> Result<(), String> {
    promote_update_with_writer(
        UpdatePromotion {
            registry_file,
            pending_file,
            current,
            installed,
            stage,
            destination,
            backup,
        },
        write_registry_at,
    )
}

fn promote_update_with_writer<F>(
    promotion: UpdatePromotion<'_>,
    persist_registry: F,
) -> Result<(), String>
where
    F: Fn(&Path, &[InstalledGame]) -> Result<(), String>,
{
    let UpdatePromotion {
        registry_file,
        pending_file,
        current,
        installed,
        stage,
        destination,
        backup,
    } = promotion;
    journal(
        pending_file,
        &current.game_slug,
        &current.release_id,
        &installed.release_id,
        UpdateJournalPhase::Activating,
    )?;
    let old_registry = read_registry_at(registry_file)?;
    activate_staged_installation(stage, destination, backup)?;
    let mut next = old_registry.clone();
    next.retain(|value| value.game_slug != current.game_slug);
    next.push(installed.clone());
    if let Err(error) = persist_registry(registry_file, &next) {
        rollback_activation(destination, backup);
        return Err(error);
    }
    if let Err(error) = journal(
        pending_file,
        &current.game_slug,
        &current.release_id,
        &installed.release_id,
        UpdateJournalPhase::RegistryPersisted,
    ) {
        let _ = write_registry_at(registry_file, &old_registry);
        rollback_activation(destination, backup);
        return Err(error);
    }
    finish_committed_update(
        || finalize_activation_backup(backup),
        || clear_pending_update(pending_file, &current.game_slug),
    );
    Ok(())
}
fn finish_committed_update<F, G>(finalize_backup: F, clear_journal: G)
where
    F: FnOnce() -> Result<(), String>,
    G: FnOnce() -> Result<(), String>,
{
    if finalize_backup().is_ok() {
        let _ = clear_journal();
    }
}

async fn full_update(
    app: &AppHandle,
    title: &str,
    execution: &UpdateExecutionPlan,
    current: &InstalledGame,
    cancellation: &AtomicBool,
    fallback: bool,
) -> Result<InstalledGame, String> {
    let target = execution.update.target().clone();
    let plan = DistributionPlan {
        game_slug: current.game_slug.clone(),
        release: target,
        manifest: execution.manifest.clone(),
        download: execution.fallback_download.clone(),
    };
    let pending = pending_updates_path(app)?;
    journal(
        &pending,
        &current.game_slug,
        &current.release_id,
        &plan.release.id,
        if fallback {
            UpdateJournalPhase::FullFallback
        } else {
            UpdateJournalPhase::Downloading
        },
    )?;
    emit_progress(
        app,
        &plan,
        title,
        if fallback {
            "full_fallback"
        } else {
            "downloading_update"
        },
        saved_download_progress(app, &plan),
        plan.download.total_size_bytes.parse().unwrap_or(0),
        None,
    );
    let archive = download_artifact(
        app,
        title,
        &plan,
        cancellation,
        if fallback {
            "full_fallback"
        } else {
            "downloading_update"
        },
    )
    .await?;
    ensure_not_cancelled(cancellation)?;
    verify_archive_checksum(&archive, &plan.download.sha256)?;
    let (destination, stage, backup) = derived_update_paths(current)?;
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .map_err(|error| format!("could not clear update staging: {error}"))?;
    }
    fs::create_dir_all(&stage)
        .map_err(|error| format!("could not create update staging: {error}"))?;
    let declared = plan
        .release
        .installed_size_bytes
        .parse::<u64>()
        .map_err(|_| "invalid installed size")?;
    extract_and_validate_archive(
        &archive,
        &stage,
        declared.saturating_add(declared / 20).max(1),
        &plan.manifest.entrypoint,
    )?;
    emit_progress(app, &plan, title, "verifying_update", 0, 0, None);
    let installed = build_installed(current, title, execution, &destination)?;
    promote_update(
        &registry_path(app)?,
        &pending,
        current,
        &installed,
        &stage,
        &destination,
        &backup,
    )?;
    Ok(installed)
}

async fn patch_update(
    app: &AppHandle,
    title: &str,
    execution: &UpdateExecutionPlan,
    current: &InstalledGame,
    cancellation: &AtomicBool,
) -> Result<InstalledGame, String> {
    let UpdatePlan::Patch { patch, .. } = &execution.update else {
        return Err("not a patch update".into());
    };
    let downloads = execution
        .patch_downloads
        .as_ref()
        .ok_or("patch download authorizations are missing")?;
    let pending = pending_updates_path(app)?;
    journal(
        &pending,
        &current.game_slug,
        &current.release_id,
        &execution.update.target().id,
        UpdateJournalPhase::Downloading,
    )?;
    let downloads_root = app_data_directory(app)?.join("downloads");
    tokio::fs::create_dir_all(&downloads_root)
        .await
        .map_err(|error| format!("could not create download directory: {error}"))?;
    let (patch_path, signature_path) = patch_payload_paths(&downloads_root, &patch.id);
    let total = downloads
        .patch
        .total_size_bytes
        .parse::<u64>()
        .map_err(|_| "invalid patch size")?;
    emit_progress(
        app,
        &DistributionPlan {
            game_slug: current.game_slug.clone(),
            release: execution.update.target().clone(),
            manifest: execution.manifest.clone(),
            download: execution.fallback_download.clone(),
        },
        title,
        "downloading_update",
        0,
        total,
        None,
    );
    download_update_file(&downloads.patch, &patch_path, cancellation, |downloaded| {
        emit_progress(
            app,
            &DistributionPlan {
                game_slug: current.game_slug.clone(),
                release: execution.update.target().clone(),
                manifest: execution.manifest.clone(),
                download: execution.fallback_download.clone(),
            },
            title,
            "downloading_update",
            downloaded,
            total,
            None,
        );
    })
    .await?;
    download_update_file(&downloads.signature, &signature_path, cancellation, |_| {}).await?;
    ensure_not_cancelled(cancellation)?;
    let (destination, stage, backup) = derived_update_paths(current)?;
    if stage.exists() {
        fs::remove_dir_all(&stage)
            .map_err(|error| format!("could not clear update staging: {error}"))?;
    }
    journal(
        &pending,
        &current.game_slug,
        &current.release_id,
        &execution.update.target().id,
        UpdateJournalPhase::Applying,
    )?;
    copy_tree(&destination, &stage, cancellation)?;
    let butler_stage = stage
        .parent()
        .ok_or("invalid update staging path")?
        .join(format!(".{}.butler", current.game_slug));
    if butler_stage.exists() {
        let _ = fs::remove_dir_all(&butler_stage);
    }
    fs::create_dir_all(&butler_stage)
        .map_err(|error| format!("could not create Butler staging: {error}"))?;
    emit_progress(
        app,
        &DistributionPlan {
            game_slug: current.game_slug.clone(),
            release: execution.update.target().clone(),
            manifest: execution.manifest.clone(),
            download: execution.fallback_download.clone(),
        },
        title,
        "applying_update",
        total,
        total,
        None,
    );
    let butler = Butler::locate(app)?;
    butler.apply(
        &patch_path,
        &signature_path,
        &stage,
        &butler_stage,
        cancellation,
    )?;
    journal(
        &pending,
        &current.game_slug,
        &current.release_id,
        &execution.update.target().id,
        UpdateJournalPhase::Verifying,
    )?;
    emit_progress(
        app,
        &DistributionPlan {
            game_slug: current.game_slug.clone(),
            release: execution.update.target().clone(),
            manifest: execution.manifest.clone(),
            download: execution.fallback_download.clone(),
        },
        title,
        "verifying_update",
        total,
        total,
        None,
    );
    butler.verify(&signature_path, &stage, cancellation)?;
    if !stage
        .join(safe_relative_path(&execution.manifest.entrypoint)?)
        .is_file()
    {
        return Err("updated installation is missing its entrypoint".into());
    }
    let installed = build_installed(current, title, execution, &destination)?;
    promote_update(
        &registry_path(app)?,
        &pending,
        current,
        &installed,
        &stage,
        &destination,
        &backup,
    )?;
    let _ = fs::remove_dir_all(butler_stage);
    remove_patch_payload_files(&patch_path, &signature_path);
    Ok(installed)
}

fn patch_failure_is_refreshable(error: &str) -> bool {
    matches!(error, DOWNLOAD_AUTHORIZATION_EXPIRED | DOWNLOAD_INTERRUPTED)
}

fn patch_failure_uses_full_fallback(error: &str, authorization_refresh_attempted: bool) -> bool {
    error != "installation cancelled"
        && (authorization_refresh_attempted || !patch_failure_is_refreshable(error))
}

#[tauri::command]
pub async fn update_game(
    app: AppHandle,
    manager: tauri::State<'_, InstallationManager>,
    title: String,
    plan: UpdateExecutionPlan,
    authorization_refresh_attempted: bool,
) -> Result<InstalledGame, InstallCommandError> {
    let game_slug = plan.update.source().id.clone();
    let registry =
        read_registry_at(&registry_path(&app).map_err(InstallCommandError::from_message)?)
            .map_err(InstallCommandError::from_message)?;
    let current = registry
        .iter()
        .find(|value| value.release_id == game_slug)
        .cloned()
        .ok_or_else(|| {
            InstallCommandError::from_message("update source is not installed".into())
        })?;
    let cancellation = manager
        .begin(&current.game_slug)
        .map_err(InstallCommandError::from_message)?;
    let result = async {
        validate_update(&current, &plan)?;
        emit_progress(
            &app,
            &DistributionPlan {
                game_slug: current.game_slug.clone(),
                release: plan.update.target().clone(),
                manifest: plan.manifest.clone(),
                download: plan.fallback_download.clone(),
            },
            &title,
            "preparing_update",
            0,
            0,
            None,
        );
        match &plan.update {
            UpdatePlan::Patch { patch, .. } => {
                match patch_update(&app, &title, &plan, &current, &cancellation).await {
                    Ok(value) => Ok(value),
                    Err(error)
                        if !patch_failure_uses_full_fallback(
                            &error,
                            authorization_refresh_attempted,
                        ) =>
                    {
                        Err(error)
                    }
                    Err(_) => {
                        cleanup_patch_attempt(&app, &current, &patch.id);
                        full_update(&app, &title, &plan, &current, &cancellation, true).await
                    }
                }
            }
            UpdatePlan::Full { .. } => {
                full_update(&app, &title, &plan, &current, &cancellation, false).await
            }
        }
    }
    .await;
    manager.finish(&current.game_slug);
    match result {
        Ok(installed) => {
            let progress_plan = DistributionPlan {
                game_slug: current.game_slug,
                release: plan.update.target().clone(),
                manifest: plan.manifest,
                download: plan.fallback_download,
            };
            emit_progress(&app, &progress_plan, &title, "installed", 0, 0, None);
            Ok(installed)
        }
        Err(error) => Err(InstallCommandError::from_message(error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn installed_fixture(root: &Path, release: &str) -> InstalledGame {
        let destination = root.join("game");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("game.exe"), release).unwrap();
        InstalledGame {
            game_slug: "game".into(),
            title: "Game".into(),
            version: if release == "source" { "1" } else { "2" }.into(),
            release_id: release.into(),
            release_number: if release == "source" { 1 } else { 2 },
            artifact_id: if release == "source" {
                "source-artifact"
            } else {
                "target-artifact"
            }
            .into(),
            installed_size_bytes: "8".into(),
            install_directory: destination.to_string_lossy().into_owned(),
            entrypoint: "game.exe".into(),
            installed_at: "1".into(),
            status: InstallationStatus::Installed,
            launch_arguments: vec![],
            working_directory: None,
            environment: HashMap::new(),
        }
    }

    fn valid_execution() -> UpdateExecutionPlan {
        let artifact_sha = "a".repeat(64);
        let patch_sha = "b".repeat(64);
        let signature_sha = "c".repeat(64);
        let target = ReleaseSummary {
            id: "target".into(),
            version: "2".into(),
            release_number: 2,
            published_at: "2026-08-28T00:00:00.000Z".into(),
            artifact_id: "target-artifact".into(),
            target: ReleaseTarget {
                platform: "WINDOWS".into(),
                architecture: "X86_64".into(),
            },
            compressed_size_bytes: "100".into(),
            installed_size_bytes: "8".into(),
            sha256: artifact_sha.clone(),
            manifest_schema_version: "1".into(),
        };
        let patch = ReleasePatch {
            id: "patch-id".into(),
            source_release_id: "source".into(),
            target_release_id: "target".into(),
            target: target.target.clone(),
            algorithm: WHARF_ALGORITHM.into(),
            format_version: WHARF_FORMAT_VERSION.into(),
            status: "READY".into(),
            patch: PatchFileDeclaration {
                size_bytes: "40".into(),
                sha256: patch_sha.clone(),
            },
            signature: PatchFileDeclaration {
                size_bytes: "8".into(),
                sha256: signature_sha.clone(),
            },
            expected_installation_sha256: signature_sha.clone(),
            generation_duration_ms: "50".into(),
            created_at: "2026-08-28T00:00:00.000Z".into(),
            updated_at: "2026-08-28T00:00:00.000Z".into(),
        };
        UpdateExecutionPlan {
            update: UpdatePlan::Patch {
                source: UpdateReleaseIdentity {
                    id: "source".into(),
                    version: "1".into(),
                    release_number: 1,
                },
                target: target.clone(),
                fallback_artifact_id: target.artifact_id.clone(),
                patch: Box::new(patch),
            },
            manifest: InstallManifest {
                schema_version: "1".into(),
                release_id: target.id.clone(),
                artifact_id: target.artifact_id.clone(),
                entrypoint: "game.exe".into(),
                launch_arguments: vec![],
                working_directory: None,
                executables: vec!["game.exe".into()],
                environment: HashMap::new(),
            },
            patch_downloads: Some(PatchDownloadAuthorizations {
                patch: PatchFileDownloadAuthorization {
                    patch_id: "patch-id".into(),
                    file: "PATCH".into(),
                    url: "https://downloads.test/patch.pwr".into(),
                    expires_at: "2026-08-28T01:00:00.000Z".into(),
                    total_size_bytes: "40".into(),
                    sha256: patch_sha,
                    etag: Some("patch-v1".into()),
                },
                signature: PatchFileDownloadAuthorization {
                    patch_id: "patch-id".into(),
                    file: "SIGNATURE".into(),
                    url: "https://downloads.test/patch.pwr.sig".into(),
                    expires_at: "2026-08-28T01:00:00.000Z".into(),
                    total_size_bytes: "8".into(),
                    sha256: signature_sha,
                    etag: Some("signature-v1".into()),
                },
            }),
            fallback_download: DownloadAuthorization {
                artifact_id: target.artifact_id,
                url: "https://downloads.test/full.zip".into(),
                expires_at: "2026-08-28T01:00:00.000Z".into(),
                total_size_bytes: "100".into(),
                sha256: artifact_sha,
                etag: Some("full-v1".into()),
            },
        }
    }

    fn current_fixture() -> InstalledGame {
        InstalledGame {
            game_slug: "game".into(),
            title: "Game".into(),
            version: "1".into(),
            release_id: "source".into(),
            release_number: 1,
            artifact_id: "source-artifact".into(),
            installed_size_bytes: "8".into(),
            install_directory: "C:\\Games\\game".into(),
            entrypoint: "game.exe".into(),
            installed_at: "1".into(),
            status: InstallationStatus::Installed,
            launch_arguments: vec![],
            working_directory: None,
            environment: HashMap::new(),
        }
    }

    #[test]
    fn validate_update_accepts_the_canonical_patch_fixture() {
        validate_update(&current_fixture(), &valid_execution()).unwrap();
    }

    #[test]
    fn validate_update_rejects_source_and_target_mismatches() {
        let current = current_fixture();
        let mut wrong_source = valid_execution();
        if let UpdatePlan::Patch { source, .. } = &mut wrong_source.update {
            source.version = "0".into();
        }
        assert!(validate_update(&current, &wrong_source)
            .unwrap_err()
            .contains("source"));

        let mut wrong_target = valid_execution();
        wrong_target.manifest.release_id = "other-target".into();
        assert!(validate_update(&current, &wrong_target)
            .unwrap_err()
            .contains("target"));
    }

    #[test]
    fn validate_update_requires_patch_to_be_exactly_n_plus_one() {
        let current = current_fixture();
        let mut skipped_target = valid_execution();
        if let UpdatePlan::Patch { target, .. } = &mut skipped_target.update {
            target.release_number = 3;
        }
        assert!(validate_update(&current, &skipped_target)
            .unwrap_err()
            .contains("not applicable"));

        let mut full = valid_execution();
        let (source, mut target, fallback_artifact_id) = match full.update {
            UpdatePlan::Patch {
                source,
                target,
                fallback_artifact_id,
                ..
            } => (source, target, fallback_artifact_id),
            UpdatePlan::Full { .. } => unreachable!(),
        };
        target.release_number = 3;
        full.update = UpdatePlan::Full {
            source,
            target,
            fallback_artifact_id,
            reason: "SOURCE_NOT_PREDECESSOR".into(),
        };
        full.patch_downloads = None;
        validate_update(&current, &full).unwrap();
    }
    #[test]
    fn validate_update_rejects_patch_status_and_signature_hash_mismatches() {
        let current = current_fixture();
        let mut failed = valid_execution();
        if let UpdatePlan::Patch { patch, .. } = &mut failed.update {
            patch.status = "FAILED".into();
        }
        assert!(validate_update(&current, &failed)
            .unwrap_err()
            .contains("not applicable"));

        let mut wrong_signature = valid_execution();
        if let UpdatePlan::Patch { patch, .. } = &mut wrong_signature.update {
            patch.expected_installation_sha256 = "d".repeat(64);
        }
        assert!(validate_update(&current, &wrong_signature)
            .unwrap_err()
            .contains("not applicable"));
    }

    #[test]
    fn validate_update_rejects_divergent_independent_downloads() {
        let current = current_fixture();
        let mut wrong_hash = valid_execution();
        wrong_hash.patch_downloads.as_mut().unwrap().patch.sha256 = "d".repeat(64);
        assert!(validate_update(&current, &wrong_hash)
            .unwrap_err()
            .contains("download declarations"));

        let mut wrong_signature_file = valid_execution();
        wrong_signature_file
            .patch_downloads
            .as_mut()
            .unwrap()
            .signature
            .file = "PATCH".into();
        assert!(validate_update(&current, &wrong_signature_file)
            .unwrap_err()
            .contains("download declarations"));

        let mut wrong_size = valid_execution();
        wrong_size
            .patch_downloads
            .as_mut()
            .unwrap()
            .patch
            .total_size_bytes = "41".into();
        assert!(validate_update(&current, &wrong_size)
            .unwrap_err()
            .contains("download declarations"));
    }

    #[test]
    fn patch_failure_refreshes_once_then_uses_full_fallback() {
        assert!(!patch_failure_uses_full_fallback(
            DOWNLOAD_AUTHORIZATION_EXPIRED,
            false
        ));
        assert!(!patch_failure_uses_full_fallback(
            DOWNLOAD_INTERRUPTED,
            false
        ));
        assert!(patch_failure_uses_full_fallback(
            DOWNLOAD_AUTHORIZATION_EXPIRED,
            true
        ));
        assert!(patch_failure_uses_full_fallback(
            "patch integrity verification failed",
            false
        ));
        assert!(!patch_failure_uses_full_fallback(
            "installation cancelled",
            true
        ));
    }

    #[test]
    fn committed_or_fallback_updates_remove_patch_payloads() {
        let directory = tempfile::tempdir().unwrap();
        let (patch, signature) = patch_payload_paths(directory.path(), "patch-id");
        fs::write(&patch, b"patch").unwrap();
        fs::write(&signature, b"signature").unwrap();

        remove_patch_payload_files(&patch, &signature);

        assert!(!patch.exists());
        assert!(!signature.exists());
        remove_patch_payload_files(&patch, &signature);
    }

    #[test]
    fn patch_failure_before_promotion_preserves_registry_and_active_destination() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let current = installed_fixture(&root, "source");
        write_registry_at(&registry_file, std::slice::from_ref(&current)).unwrap();

        assert!(patch_failure_uses_full_fallback("corrupt patch", false));
        assert_eq!(
            fs::read_to_string(root.join("game/game.exe")).unwrap(),
            "source"
        );
        assert_eq!(
            read_registry_at(&registry_file).unwrap()[0].release_id,
            "source"
        );
    }

    #[test]
    fn registry_failure_restores_backup_and_source_registry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let pending_file = directory.path().join(PENDING_UPDATES_FILE);
        let current = installed_fixture(&root, "source");
        write_registry_at(&registry_file, std::slice::from_ref(&current)).unwrap();
        let stage = root.join(".game.staging");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("game.exe"), "target").unwrap();
        let mut target = current.clone();
        target.release_id = "target".into();
        target.release_number = 2;
        target.version = "2".into();
        let destination = root.join("game");
        let backup = root.join(".game.backup");

        let error = promote_update_with_writer(
            UpdatePromotion {
                registry_file: &registry_file,
                pending_file: &pending_file,
                current: &current,
                installed: &target,
                stage: &stage,
                destination: &destination,
                backup: &backup,
            },
            |_, _| Err("simulated registry failure".into()),
        )
        .unwrap_err();

        assert_eq!(error, "simulated registry failure");
        assert_eq!(
            fs::read_to_string(destination.join("game.exe")).unwrap(),
            "source"
        );
        assert_eq!(
            read_registry_at(&registry_file).unwrap()[0].release_id,
            "source"
        );
        assert!(!backup.exists());
    }

    #[test]
    fn post_registry_cleanup_failure_remains_committed_for_recovery() {
        use std::cell::Cell;
        let journal_cleared = Cell::new(false);
        finish_committed_update(
            || Err("simulated backup cleanup failure".into()),
            || {
                journal_cleared.set(true);
                Ok(())
            },
        );
        assert!(!journal_cleared.get());
    }
    #[test]
    fn healthy_update_persists_target_before_removing_backup() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let pending_file = directory.path().join(PENDING_UPDATES_FILE);
        let current = installed_fixture(&root, "source");
        write_registry_at(&registry_file, std::slice::from_ref(&current)).unwrap();
        let stage = root.join(".game.staging");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("game.exe"), "target").unwrap();
        let mut target = current.clone();
        target.release_id = "target".into();
        target.release_number = 2;
        target.version = "2".into();
        let destination = root.join("game");
        let backup = root.join(".game.backup");

        promote_update(
            &registry_file,
            &pending_file,
            &current,
            &target,
            &stage,
            &destination,
            &backup,
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(destination.join("game.exe")).unwrap(),
            "target"
        );
        assert_eq!(
            read_registry_at(&registry_file).unwrap()[0].release_id,
            "target"
        );
        assert!(!backup.exists());
        assert!(read_json_or_default::<Vec<PendingUpdate>>(&pending_file)
            .unwrap()
            .is_empty());
    }
    #[test]
    fn manager_blocks_update_while_game_is_running() {
        let manager = InstallationManager::new();
        manager
            .state
            .lock()
            .unwrap()
            .running_games
            .insert("game".into());
        assert_eq!(manager.begin("game").unwrap_err(), GAME_RUNNING);
    }

    #[tokio::test]
    async fn patch_resume_uses_shared_downloader_range_and_etag() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0_u8; 2048];
            let count = stream.read(&mut request).unwrap();
            let request = String::from_utf8_lossy(&request[..count]).to_ascii_lowercase();
            assert!(request.contains("range: bytes=4-"));
            assert!(request.contains("if-range: patch-v2"));
            stream.write_all(
                b"HTTP/1.1 206 Partial Content\r\nContent-Length: 4\r\nContent-Range: bytes 4-7/8\r\nETag: patch-v2\r\nConnection: close\r\n\r\nfold",
            ).unwrap();
        });
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("patch-id.pwr.part");
        fs::write(&path, b"mani").unwrap();
        let cancellation = AtomicBool::new(false);

        download_to_file(
            &reqwest::Client::new(),
            &format!("http://{address}/patch.pwr?authorization=second"),
            &path,
            8,
            Some("patch-v2"),
            &cancellation,
            |_| {},
        )
        .await
        .unwrap();

        server.join().unwrap();
        assert_eq!(fs::read(path).unwrap(), b"manifold");
    }

    #[test]
    fn recovery_rolls_back_every_pre_registry_phase_and_finalizes_persisted_registry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let pending_file = directory.path().join(PENDING_UPDATES_FILE);
        for phase in [
            UpdateJournalPhase::Preparing,
            UpdateJournalPhase::Downloading,
            UpdateJournalPhase::Applying,
            UpdateJournalPhase::Verifying,
            UpdateJournalPhase::FullFallback,
        ] {
            let stage = root.join(".game.staging");
            fs::create_dir_all(&stage).unwrap();
            let game = installed_fixture(&root, "source");
            write_registry_at(&registry_file, &[game]).unwrap();
            journal(&pending_file, "game", "source", "target", phase).unwrap();
            recover_pending_updates_at(&registry_file, &pending_file).unwrap();
            assert!(!stage.exists());
        }
    }

    #[test]
    fn recovery_restores_backup_when_activation_crashes_before_registry() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let pending_file = directory.path().join(PENDING_UPDATES_FILE);
        let game = installed_fixture(&root, "source");
        write_registry_at(&registry_file, &[game]).unwrap();
        fs::rename(root.join("game"), root.join(".game.backup")).unwrap();
        fs::create_dir(root.join("game")).unwrap();
        fs::write(root.join("game/game.exe"), "target").unwrap();
        journal(
            &pending_file,
            "game",
            "source",
            "target",
            UpdateJournalPhase::Activating,
        )
        .unwrap();
        recover_pending_updates_at(&registry_file, &pending_file).unwrap();
        assert_eq!(
            fs::read_to_string(root.join("game/game.exe")).unwrap(),
            "source"
        );
    }

    #[test]
    fn recovery_keeps_new_install_only_after_target_registry_is_persisted() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("games");
        fs::create_dir_all(&root).unwrap();
        let registry_file = directory.path().join("installations.json");
        let pending_file = directory.path().join(PENDING_UPDATES_FILE);
        let old = installed_fixture(&root, "source");
        fs::rename(root.join("game"), root.join(".game.backup")).unwrap();
        let target = installed_fixture(&root, "target");
        write_registry_at(&registry_file, &[target]).unwrap();
        journal(
            &pending_file,
            "game",
            &old.release_id,
            "target",
            UpdateJournalPhase::RegistryPersisted,
        )
        .unwrap();
        recover_pending_updates_at(&registry_file, &pending_file).unwrap();
        assert!(!root.join(".game.backup").exists());
        assert_eq!(
            fs::read_to_string(root.join("game/game.exe")).unwrap(),
            "target"
        );
    }
}
