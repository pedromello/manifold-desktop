import {
  type FormEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useParams } from 'react-router-dom';
import {
  findStoredPublisherRelease,
  isPublisherError,
  parentDirectory,
  PublishManifest,
  publisherAdapter,
  PublisherAdapter,
  PublisherGame,
  PublisherStudio,
  storePublisherRelease,
  StoredPublisherRelease,
} from './publishing';
import { ReleaseDataForm } from './studio';

type ReleaseWizardRouteProps = {
  studios: PublisherStudio[];
  adapter?: PublisherAdapter;
  onUnauthorized: () => void;
};

const phaseStep = {
  details: 0,
  file: 1,
  preflight: 2,
  manifest: 3,
  uploading: 4,
  verifying: 5,
  success: 6,
} as const;

function humanBytes(value: string, locale: string) {
  const bytes = Number(value);
  if (!Number.isFinite(bytes)) return value;
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  let amount = bytes;
  let unit = 0;
  while (amount >= 1024 && unit < units.length - 1) {
    amount /= 1024;
    unit += 1;
  }
  return `${new Intl.NumberFormat(locale, {
    maximumFractionDigits: unit === 0 ? 0 : 1,
  }).format(amount)} ${units[unit]}`;
}

function errorKey(error: unknown) {
  if (!isPublisherError(error)) return 'publisher.errors.unavailable';
  if (error.code === 'AUTHENTICATION_REQUIRED')
    return 'publisher.errors.authentication';
  if (error.code === 'PERMISSION_DENIED' || error.code === 'FORBIDDEN')
    return 'publisher.errors.permission';
  if (error.code === 'UPLOAD_CANCELLED') return 'publisher.errors.cancelled';
  if (error.code === 'INVALID_ARCHIVE') return 'publisher.errors.archive';
  return 'publisher.errors.unavailable';
}

function WizardSteps({ current }: { current: number }) {
  const { t } = useTranslation();
  const labels = ['details', 'file', 'review', 'publish'];
  const complete = current === 6;
  const macroCurrent =
    current === 0 ? 0 : current <= 2 ? 1 : current === 3 ? 2 : 3;
  return (
    <div className="wizard-progress">
      <span className="wizard-step-count">
        {complete
          ? t('publisher.completed')
          : t('publisher.stepCount', {
              current: macroCurrent + 1,
              total: labels.length,
            })}
      </span>
      <ol className="wizard-steps" aria-label={t('publisher.progress')}>
        {labels.map((label, index) => (
          <li
            className={
              index < macroCurrent || complete ? 'complete' : undefined
            }
            aria-current={
              !complete && index === macroCurrent ? 'step' : undefined
            }
            key={label}
          >
            <span aria-hidden="true">
              {index < macroCurrent || complete ? '✓' : index + 1}
            </span>
            <strong>{t(`publisher.steps.${label}`)}</strong>
          </li>
        ))}
      </ol>
    </div>
  );
}

export function ReleaseWizardRoute({
  studios,
  adapter = publisherAdapter,
  onUnauthorized,
}: ReleaseWizardRouteProps) {
  const { t } = useTranslation();
  const { studioSlug, gameSlug, releaseId } = useParams();
  const [game, setGame] = useState<PublisherGame | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState(false);
  const studio = studios.find((value) => value.slug === studioSlug);

  useEffect(() => {
    if (!studioSlug || !gameSlug) {
      setLoadError(true);
      setLoading(false);
      return;
    }
    let active = true;
    adapter
      .listGames(studioSlug)
      .then((games) => {
        if (!active) return;
        const value = games.find((candidate) => candidate.slug === gameSlug);
        setGame(value ?? null);
        setLoadError(!value);
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (
          isPublisherError(error) &&
          error.code === 'AUTHENTICATION_REQUIRED'
        ) {
          onUnauthorized();
        }
        setLoadError(true);
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [adapter, gameSlug, onUnauthorized, studioSlug]);

  if (loading) {
    return (
      <section className="publisher-page">
        <div className="catalog-state" role="status">
          <span className="spinner" />
          <strong>{t('publisher.loading')}</strong>
        </div>
      </section>
    );
  }

  if (loadError || !studio || !game || !studioSlug) {
    return (
      <section className="publisher-page">
        <div className="catalog-state error-state" role="alert">
          <span>{t('publisher.loadError')}</span>
          <Link className="secondary-link" to="/studio">
            {t('publisher.backToStudio')}
          </Link>
        </div>
      </section>
    );
  }

  return (
    <section className="publisher-page" aria-labelledby="publisher-title">
      <Link
        className="back-link"
        to={`/studio/${studioSlug}/games/${gameSlug}`}
      >
        ← {game.title}
      </Link>
      <header className="page-header">
        <span className="eyebrow">{t('publisher.eyebrow')}</span>
        <h1 id="publisher-title">{t('publisher.title')}</h1>
        <p>{t('publisher.intro', { game: game.title })}</p>
      </header>
      {!releaseId ? (
        <>
          <WizardSteps current={0} />
          <div className="wizard-card">
            <div className="wizard-heading">
              <div>
                <h2>{t('publisher.dataTitle')}</h2>
                <p>{t('publisher.dataHelp')}</p>
              </div>
            </div>
            <ReleaseDataForm
              adapter={adapter}
              game={game}
              studioSlug={studioSlug}
            />
          </div>
        </>
      ) : (
        <ReleaseArtifactWizard adapter={adapter} releaseId={releaseId} />
      )}
    </section>
  );
}

function ReleaseArtifactWizard({
  adapter,
  releaseId,
}: {
  adapter: PublisherAdapter;
  releaseId: string;
}) {
  const { t, i18n } = useTranslation();
  const [stored, setStored] = useState<StoredPublisherRelease | null>(
    () => findStoredPublisherRelease(releaseId) ?? null,
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [draftVersion, setDraftVersion] = useState(
    () => findStoredPublisherRelease(releaseId)?.release.version ?? '',
  );
  const [draftNotes, setDraftNotes] = useState(
    () => findStoredPublisherRelease(releaseId)?.release.releaseNotes ?? '',
  );
  const [uploadedBytes, setUploadedBytes] = useState(0);
  const [totalBytes, setTotalBytes] = useState(0);
  const [attempt, setAttempt] = useState(1);
  const headingRef = useRef<HTMLHeadingElement>(null);

  const update = useCallback(
    (transform: (value: StoredPublisherRelease) => StoredPublisherRelease) => {
      setStored((value) => {
        if (!value) return value;
        const next = transform(value);
        storePublisherRelease(next);
        return next;
      });
    },
    [],
  );

  useEffect(() => {
    headingRef.current?.focus();
  }, [stored?.phase]);

  const currentStep = stored ? phaseStep[stored.phase] : 1;
  const manifest = stored?.manifest;
  const inspection = stored?.inspection;

  const percent = useMemo(
    () =>
      totalBytes > 0
        ? Math.min(100, Math.round((uploadedBytes / totalBytes) * 100))
        : 0,
    [totalBytes, uploadedBytes],
  );

  if (!stored) {
    return (
      <div className="catalog-state error-state" role="alert">
        <span>{t('publisher.recoveryMissingTitle')}</span>
        <strong>{t('publisher.recoveryMissingHelp')}</strong>
      </div>
    );
  }

  function returnToDetails() {
    if (!stored) return;
    setDraftVersion(stored.release.version);
    setDraftNotes(stored.release.releaseNotes ?? '');
    setError(null);
    update((value) => ({ ...value, phase: 'details' }));
  }

  async function saveDraft(event: FormEvent) {
    event.preventDefault();
    if (!stored) return;
    setBusy(true);
    setError(null);
    try {
      const release = await adapter.updateDraft(
        stored.gameSlug,
        stored.release.id,
        draftVersion.trim(),
        draftNotes.trim() || null,
      );
      update((value) => ({ ...value, release, phase: 'file' }));
    } catch (nextError) {
      setError(errorKey(nextError));
    } finally {
      setBusy(false);
    }
  }

  async function selectArchive() {
    setError(null);
    try {
      const archivePath = await adapter.selectArchive();
      if (!archivePath) return;
      update((value) => ({
        ...value,
        archivePath,
        inspection: null,
        manifest: null,
        phase: 'file',
      }));
    } catch {
      setError('publisher.errors.filePicker');
    }
  }

  async function inspectArchive() {
    const archivePath = stored?.archivePath;
    if (!archivePath) return;
    setBusy(true);
    setError(null);
    update((value) => ({ ...value, phase: 'preflight' }));
    try {
      const nextInspection = await adapter.inspectArchive(archivePath);
      if (!nextInspection.suggestedEntrypoint) {
        setError('publisher.errors.noExecutable');
        update((value) => ({
          ...value,
          inspection: nextInspection,
          phase: 'file',
        }));
        return;
      }
      const nextManifest: PublishManifest = {
        schemaVersion: '1',
        entrypoint: nextInspection.suggestedEntrypoint,
        launchArguments: [],
        workingDirectory: nextInspection.suggestedWorkingDirectory,
        executables: nextInspection.executables,
        environment: {},
      };
      update((value) => ({
        ...value,
        inspection: nextInspection,
        manifest: nextManifest,
        phase: 'manifest',
      }));
    } catch (nextError) {
      setError(errorKey(nextError));
      update((value) => ({ ...value, phase: 'file' }));
    } finally {
      setBusy(false);
    }
  }

  function selectEntrypoint(entrypoint: string) {
    update((value) => ({
      ...value,
      manifest: value.manifest
        ? {
            ...value.manifest,
            entrypoint,
            workingDirectory: parentDirectory(entrypoint),
          }
        : null,
    }));
  }

  async function publish() {
    if (!stored?.archivePath || !stored.manifest) return;
    const {
      archivePath,
      inspection: currentInspection,
      manifest,
      release,
    } = stored;
    setBusy(true);
    setError(null);
    setUploadedBytes(0);
    setTotalBytes(Number(currentInspection?.compressedSizeBytes ?? 0));
    update((value) => ({
      ...value,
      phase: 'uploading',
      uploadStarted: true,
    }));
    try {
      const confirmation = await adapter.publish(
        release.id,
        archivePath,
        manifest,
        (progress) => {
          setUploadedBytes(progress.uploadedBytes);
          setTotalBytes(progress.totalBytes);
          setAttempt(progress.attempt);
          if (progress.phase === 'verifying') {
            update((value) => ({ ...value, phase: 'verifying' }));
          }
        },
      );
      update((value) => ({
        ...value,
        release: confirmation.release,
        phase: 'success',
      }));
    } catch (nextError) {
      setError(errorKey(nextError));
      update((value) => ({ ...value, phase: 'manifest' }));
    } finally {
      setBusy(false);
    }
  }

  async function cancel() {
    if (!stored) return;
    try {
      await adapter.cancel(stored.release.id);
      setError('publisher.errors.cancelled');
      update((value) => ({ ...value, phase: 'manifest' }));
    } catch {
      setError('publisher.errors.cancelFailed');
    }
  }

  return (
    <>
      <WizardSteps current={currentStep} />
      <div className="publisher-workspace">
        <div className="publisher-main">
          {stored.phase === 'details' && (
            <div className="wizard-card">
              <div className="wizard-heading">
                <div>
                  <h2 ref={headingRef} tabIndex={-1}>
                    {t('publisher.dataTitle')}
                  </h2>
                  <p>{t('publisher.dataHelp')}</p>
                </div>
              </div>
              <form
                className="release-form"
                onSubmit={(event) => void saveDraft(event)}
              >
                <label>
                  <strong>{t('publisher.version')}</strong>
                  <span>{t('publisher.versionHelp')}</span>
                  <input
                    aria-label={t('publisher.version')}
                    autoFocus
                    maxLength={50}
                    required
                    value={draftVersion}
                    onChange={(event) => setDraftVersion(event.target.value)}
                    placeholder="1.0.0"
                  />
                </label>
                <label>
                  <strong>{t('publisher.releaseNotes')}</strong>
                  <span>{t('publisher.releaseNotesHelp')}</span>
                  <textarea
                    aria-label={t('publisher.releaseNotes')}
                    maxLength={100_000}
                    rows={6}
                    value={draftNotes}
                    onChange={(event) => setDraftNotes(event.target.value)}
                  />
                </label>
                <div className="wizard-actions">
                  <button className="game-action" disabled={busy} type="submit">
                    {busy
                      ? t('publisher.creatingDraft')
                      : t('publisher.createDraft')}
                  </button>
                </div>
              </form>
            </div>
          )}

          {(stored.phase === 'file' || stored.phase === 'preflight') && (
            <div className="wizard-card">
              <div className="wizard-heading">
                <div>
                  <h2 ref={headingRef} tabIndex={-1}>
                    {stored.phase === 'preflight'
                      ? t('publisher.preflightTitle')
                      : t('publisher.fileTitle')}
                  </h2>
                  <p>
                    {stored.phase === 'preflight'
                      ? t('publisher.preflightHelp')
                      : t('publisher.fileHelp')}
                  </p>
                </div>
              </div>
              {stored.archivePath && (
                <div className="selected-file">
                  <span aria-hidden="true">ZIP</span>
                  <div>
                    <strong>
                      {stored.archivePath.split(/[\\/]/).at(-1) ??
                        stored.archivePath}
                    </strong>
                    <small>{t('publisher.fileSelected')}</small>
                  </div>
                </div>
              )}
              {stored.phase === 'preflight' ? (
                <div className="analysis-state" role="status">
                  <span className="spinner" />
                  <div>
                    <strong>{t('publisher.analyzing')}</strong>
                    <span>{t('publisher.analyzingHelp')}</span>
                  </div>
                </div>
              ) : (
                <div className="wizard-actions">
                  <button
                    className="secondary-action"
                    disabled={busy}
                    type="button"
                    onClick={returnToDetails}
                  >
                    {t('publisher.back')}
                  </button>
                  <button
                    className="secondary-action"
                    disabled={busy}
                    type="button"
                    onClick={() => void selectArchive()}
                  >
                    {stored.archivePath
                      ? t('publisher.changeFile')
                      : t('publisher.chooseFile')}
                  </button>
                  <button
                    className="game-action"
                    disabled={!stored.archivePath || busy}
                    type="button"
                    onClick={() => void inspectArchive()}
                  >
                    {t('publisher.analyze')}
                  </button>
                </div>
              )}
            </div>
          )}

          {stored.phase === 'manifest' && inspection && manifest && (
            <div className="wizard-card">
              <div className="wizard-heading">
                <div>
                  <h2 ref={headingRef} tabIndex={-1}>
                    {t('publisher.manifestTitle')}
                  </h2>
                  <p>{t('publisher.manifestHelp')}</p>
                </div>
              </div>
              <div className="file-overview">
                <div>
                  <span>{t('publisher.archiveSize')}</span>
                  <strong>
                    {humanBytes(inspection.compressedSizeBytes, i18n.language)}
                  </strong>
                </div>
                <div>
                  <span>{t('publisher.installedSize')}</span>
                  <strong>
                    {humanBytes(inspection.installedSizeBytes, i18n.language)}
                  </strong>
                </div>
              </div>
              <label className="manifest-field">
                <strong>{t('publisher.entrypoint')}</strong>
                <span>{t('publisher.entrypointHelp')}</span>
                <select
                  disabled={stored.uploadStarted}
                  value={manifest.entrypoint}
                  onChange={(event) => selectEntrypoint(event.target.value)}
                >
                  {inspection.executables.map((executable) => (
                    <option key={executable} value={executable}>
                      {executable}
                    </option>
                  ))}
                </select>
              </label>
              <details className="technical-details">
                <summary>{t('publisher.technicalDetails')}</summary>
                <dl className="manifest-review">
                  <dt>{t('publisher.executables')}</dt>
                  <dd>{inspection.executables.length}</dd>
                  <dt>{t('publisher.integrity')}</dt>
                  <dd>{t('publisher.shaReady')}</dd>
                  <dt>{t('publisher.workingDirectory')}</dt>
                  <dd>
                    {manifest.workingDirectory || t('publisher.archiveRoot')}
                  </dd>
                  <dt>{t('publisher.launchArguments')}</dt>
                  <dd>{t('publisher.none')}</dd>
                  <dt>{t('publisher.environment')}</dt>
                  <dd>{t('publisher.none')}</dd>
                </dl>
              </details>
              <div className="approval-line">
                <span aria-hidden="true">✓</span>
                <div>
                  <strong>{t('publisher.readyToSend')}</strong>
                  <span>
                    {stored.uploadStarted
                      ? t('publisher.uploadLocked')
                      : t('publisher.securityCheckedHelp')}
                  </span>
                </div>
              </div>
              <div className="wizard-actions">
                <button
                  className="secondary-action"
                  disabled={stored.uploadStarted}
                  type="button"
                  onClick={() =>
                    update((value) => ({
                      ...value,
                      phase: 'file',
                      inspection: null,
                      manifest: null,
                    }))
                  }
                >
                  {t('publisher.back')}
                </button>
                <button
                  className="game-action"
                  disabled={busy}
                  type="button"
                  onClick={() => void publish()}
                >
                  {stored.uploadStarted
                    ? t('publisher.retryUpload')
                    : t('publisher.uploadAndPublish')}
                </button>
              </div>
            </div>
          )}

          {(stored.phase === 'uploading' || stored.phase === 'verifying') && (
            <div className="wizard-card upload-card">
              <div className="wizard-heading">
                <div>
                  <h2 ref={headingRef} tabIndex={-1}>
                    {stored.phase === 'verifying'
                      ? t('publisher.verifyingTitle')
                      : t('publisher.uploadTitle')}
                  </h2>
                  <p>
                    {stored.phase === 'verifying'
                      ? t('publisher.verifyingHelp')
                      : t('publisher.uploadHelp')}
                  </p>
                </div>
              </div>
              {stored.phase === 'uploading' ? (
                <>
                  <div className="upload-meter-row">
                    <span>{inspection?.fileName}</span>
                    <strong>{percent}%</strong>
                  </div>
                  <div
                    className="install-progress upload-progress"
                    role="progressbar"
                    aria-label={t('publisher.uploadProgress')}
                    aria-valuemin={0}
                    aria-valuemax={100}
                    aria-valuenow={percent}
                  >
                    <span style={{ width: `${percent}%` }} />
                  </div>
                  <div className="upload-meter-row secondary">
                    <span>
                      {t('publisher.bytesUploaded', {
                        uploaded: humanBytes(
                          String(uploadedBytes),
                          i18n.language,
                        ),
                        total: humanBytes(String(totalBytes), i18n.language),
                      })}
                    </span>
                    {attempt > 1 && (
                      <span>{t('publisher.retryAttempt', { attempt })}</span>
                    )}
                  </div>
                  <button
                    className="secondary-action cancel-upload"
                    type="button"
                    onClick={() => void cancel()}
                  >
                    {t('publisher.cancelUpload')}
                  </button>
                </>
              ) : (
                <div className="analysis-state" role="status">
                  <span className="spinner" />
                  <div>
                    <strong>{t('publisher.verifyingStorage')}</strong>
                    <span>{t('publisher.verifyingStorageHelp')}</span>
                  </div>
                </div>
              )}
            </div>
          )}

          {stored.phase === 'success' && (
            <div className="wizard-card success-card">
              <span className="success-mark" aria-hidden="true">
                ✓
              </span>
              <h2 ref={headingRef} tabIndex={-1}>
                {t('publisher.successTitle')}
              </h2>
              <p>
                {t('publisher.successHelp', {
                  version: stored.release.version,
                  game: stored.gameTitle,
                })}
              </p>
              <p className="success-meta">
                {stored.release.version} · #{stored.release.releaseNumber} ·{' '}
                {t('publisher.published')}
              </p>
              <Link
                className="primary-link"
                to={`/studio/${stored.studioSlug}/games/${stored.gameSlug}`}
              >
                {t('publisher.backToReleases', { game: stored.gameTitle })}
              </Link>
            </div>
          )}

          {error && (
            <p className="form-notice error wizard-error" role="alert">
              {t(error)}
            </p>
          )}
        </div>
        <aside
          className="release-sidebar"
          aria-label={t('publisher.releaseSummary')}
        >
          <span className="sidebar-label">{t('publisher.releaseSummary')}</span>
          <h2>{stored.gameTitle}</h2>
          <dl>
            <div>
              <dt>{t('publisher.version')}</dt>
              <dd>{stored.release.version}</dd>
            </div>
            <div>
              <dt>{t('publisher.release')}</dt>
              <dd>#{stored.release.releaseNumber}</dd>
            </div>
            <div>
              <dt>{t('publisher.platform')}</dt>
              <dd>Windows · 64 bits</dd>
            </div>
          </dl>
          <span
            className={`release-sidebar-status ${
              stored.release.status === 'PUBLISHED' ? 'published' : ''
            }`}
          >
            {stored.release.status === 'PUBLISHED'
              ? t('publisher.published')
              : t('studio.status.draft')}
          </span>
        </aside>
      </div>
    </>
  );
}
