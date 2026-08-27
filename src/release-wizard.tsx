import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
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
  const labels = [
    'data',
    'file',
    'preflight',
    'manifest',
    'upload',
    'verify',
    'success',
  ];
  return (
    <ol className="wizard-steps" aria-label={t('publisher.progress')}>
      {labels.map((label, index) => (
        <li
          className={index < current ? 'complete' : undefined}
          aria-current={index === current ? 'step' : undefined}
          key={label}
        >
          <span>{index < current ? '✓' : index + 1}</span>
          <strong>{t(`publisher.steps.${label}`)}</strong>
        </li>
      ))}
    </ol>
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
              <span>1</span>
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
      <div
        className="release-context"
        aria-label={t('publisher.releaseSummary')}
      >
        <div>
          <span>{t('publisher.version')}</span>
          <strong>{stored.release.version}</strong>
        </div>
        <div>
          <span>{t('publisher.release')}</span>
          <strong>#{stored.release.releaseNumber}</strong>
        </div>
        <div>
          <span>{t('publisher.platform')}</span>
          <strong>Windows · x86-64 · ZIP</strong>
        </div>
        <span className="status-badge status-draft">
          {stored.release.status}
        </span>
      </div>

      {(stored.phase === 'file' || stored.phase === 'preflight') && (
        <div className="wizard-card">
          <div className="wizard-heading">
            <span>{stored.phase === 'preflight' ? 3 : 2}</span>
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
            <span>4</span>
            <div>
              <h2 ref={headingRef} tabIndex={-1}>
                {t('publisher.manifestTitle')}
              </h2>
              <p>{t('publisher.manifestHelp')}</p>
            </div>
          </div>
          <div className="preflight-summary">
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
            <div>
              <span>{t('publisher.executables')}</span>
              <strong>{inspection.executables.length}</strong>
            </div>
            <div>
              <span>{t('publisher.integrity')}</span>
              <strong>{t('publisher.shaReady')}</strong>
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
          <dl className="manifest-review">
            <dt>{t('publisher.workingDirectory')}</dt>
            <dd>{manifest.workingDirectory || t('publisher.archiveRoot')}</dd>
            <dt>{t('publisher.launchArguments')}</dt>
            <dd>{t('publisher.none')}</dd>
            <dt>{t('publisher.environment')}</dt>
            <dd>{t('publisher.none')}</dd>
          </dl>
          <div className="security-note">
            <strong>{t('publisher.securityChecked')}</strong>
            <span>
              {stored.uploadStarted
                ? t('publisher.uploadLocked')
                : t('publisher.securityCheckedHelp')}
            </span>
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
              {t('publisher.changeFile')}
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
            <span>{stored.phase === 'verifying' ? 6 : 5}</span>
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
                    uploaded: humanBytes(String(uploadedBytes), i18n.language),
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
          <span className="eyebrow">{t('publisher.successEyebrow')}</span>
          <h2 ref={headingRef} tabIndex={-1}>
            {t('publisher.successTitle')}
          </h2>
          <p>
            {t('publisher.successHelp', {
              version: stored.release.version,
              game: stored.gameTitle,
            })}
          </p>
          <div className="published-summary">
            <div>
              <span>{t('publisher.version')}</span>
              <strong>{stored.release.version}</strong>
            </div>
            <div>
              <span>{t('publisher.release')}</span>
              <strong>#{stored.release.releaseNumber}</strong>
            </div>
            <div>
              <span>{t('publisher.statusLabel')}</span>
              <strong>{t('publisher.published')}</strong>
            </div>
          </div>
          <Link
            className="primary-link"
            to={`/studio/${stored.studioSlug}/games/${stored.gameSlug}`}
          >
            {t('publisher.backToReleases')}
          </Link>
        </div>
      )}

      {error && (
        <p className="form-notice error wizard-error" role="alert">
          {t(error)}
        </p>
      )}
    </>
  );
}
