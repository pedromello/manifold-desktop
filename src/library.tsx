import { invoke } from '@tauri-apps/api/core';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { AuthPanel, AuthUser } from './auth';
import { distributionErrorKey } from './distribution-errors';
import {
  normalizeUninstallFailure,
  UninstallErrorCode,
  useInstallations,
} from './installation';

type LibraryOutlet = {
  id: string;
  slug: string | null;
  name: string;
  logoUrl: string | null;
};

export type LibraryGame = {
  libraryId: string;
  id: string;
  slug: string;
  title: string;
  description: string;
  developerName: string;
  bannerUrl: string | null;
  iconUrl: string | null;
  acquiredAt: string;
  outlet: LibraryOutlet | null;
  acquisitionLabel: string;
  acquisitionType?: 'OUTLET' | 'MANIFOLD_STORE' | 'GRANT';
};

type LibraryCatalog = { games: LibraryGame[]; total: number };

type LibraryPageProps = {
  user: AuthUser | null;
  onAuthenticated: (user: AuthUser) => void;
  onSessionExpired: () => void;
};

function readableDate(value: string, locale: string) {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return null;
  return new Intl.DateTimeFormat(locale, {
    day: 'numeric',
    month: 'short',
    year: 'numeric',
  }).format(date);
}

function uninstallErrorKey(code: UninstallErrorCode) {
  switch (code) {
    case 'GAME_RUNNING':
      return 'library.uninstallGameRunning';
    case 'INSTALLATION_ACTIVE':
      return 'library.uninstallInstallationActive';
    case 'UNSAFE_INSTALLATION_PATH':
      return 'library.uninstallUnsafePath';
    case 'LOCAL_STATE_ERROR':
      return 'library.uninstallLocalStateError';
    case 'FILESYSTEM_ERROR':
      return 'library.uninstallFilesystemError';
    case 'NOT_INSTALLED':
      return 'library.uninstallNotInstalled';
    default:
      return 'library.uninstallFailed';
  }
}

export function LibraryPage({
  user,
  onAuthenticated,
  onSessionExpired,
}: LibraryPageProps) {
  const { t, i18n } = useTranslation();
  const {
    availableUpdates,
    checkForUpdates,
    install,
    installed,
    launch,
    playing,
    progress,
    uninstall,
  } = useInstallations();
  const [catalog, setCatalog] = useState<LibraryCatalog | null>(null);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(Boolean(user));
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const [launchingSlug, setLaunchingSlug] = useState<string | null>(null);
  const [launchErrors, setLaunchErrors] = useState<Record<string, boolean>>({});
  const [uninstallTarget, setUninstallTarget] = useState<LibraryGame | null>(
    null,
  );
  const [uninstallingSlug, setUninstallingSlug] = useState<string | null>(null);
  const [uninstallError, setUninstallError] =
    useState<UninstallErrorCode | null>(null);
  const [uninstallSuccess, setUninstallSuccess] = useState<string | null>(null);
  const cancelUninstallRef = useRef<HTMLButtonElement>(null);
  const uninstallDialogRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!uninstallTarget) return;
    cancelUninstallRef.current?.focus();
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && !uninstallingSlug) {
        setUninstallTarget(null);
        setUninstallError(null);
      }
      if (event.key === 'Tab') {
        const buttons = Array.from(
          uninstallDialogRef.current?.querySelectorAll<HTMLButtonElement>(
            'button:not(:disabled)',
          ) ?? [],
        );
        if (buttons.length === 0) return;
        const first = buttons[0];
        const last = buttons[buttons.length - 1];
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault();
          last.focus();
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault();
          first.focus();
        }
      }
    };
    window.addEventListener('keydown', closeOnEscape);
    return () => window.removeEventListener('keydown', closeOnEscape);
  }, [uninstallTarget, uninstallingSlug]);

  useEffect(() => {
    if (!user) {
      setCatalog(null);
      setLoading(false);
      return;
    }
    let active = true;
    setLoading(true);
    setError(null);
    invoke<LibraryCatalog>('list_library')
      .then((result) => {
        if (active) setCatalog(result);
      })
      .catch((reason: unknown) => {
        if (!active) return;
        const message = typeof reason === 'string' ? reason : '';
        setError(t('library.unavailable'));
        if (/session|permission|forbidden|unauthorized/i.test(message)) {
          onSessionExpired();
        }
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [onSessionExpired, t, user, version]);

  const games = useMemo(() => {
    const normalized = query.trim().toLocaleLowerCase();
    if (!normalized) return catalog?.games ?? [];
    return (catalog?.games ?? []).filter((game) =>
      [game.title, game.developerName, game.outlet?.name]
        .filter(Boolean)
        .some((value) => value?.toLocaleLowerCase().includes(normalized)),
    );
  }, [catalog, query]);

  useEffect(() => {
    const installedSlugs = (catalog?.games ?? [])
      .map((game) => game.slug)
      .filter((slug) => installed[slug]?.status === 'INSTALLED');
    void checkForUpdates(installedSlugs);
  }, [catalog, checkForUpdates, installed]);

  if (!user) {
    return <AuthPanel onAuthenticated={onAuthenticated} />;
  }

  async function handleGameAction(game: LibraryGame) {
    const installation = installed[game.slug];
    const needsRepair = installation?.status === 'REPAIR_NEEDED';
    const updateVersion = availableUpdates[game.slug];
    if (playing[game.slug]) return;
    setLaunchErrors((current) => ({ ...current, [game.slug]: false }));
    if (installation && !needsRepair && !updateVersion) {
      setLaunchingSlug(game.slug);
      try {
        await launch(game.slug);
      } catch {
        setLaunchErrors((current) => ({ ...current, [game.slug]: true }));
      } finally {
        setLaunchingSlug(null);
      }
      return;
    }
    await install(game.slug, game.title);
  }

  async function confirmUninstall() {
    if (!uninstallTarget) return;
    setUninstallError(null);
    setUninstallSuccess(null);
    setUninstallingSlug(uninstallTarget.slug);
    try {
      await uninstall(uninstallTarget.slug);
      setUninstallSuccess(
        t('library.uninstallSuccess', { title: uninstallTarget.title }),
      );
      setUninstallTarget(null);
    } catch (reason) {
      setUninstallError(normalizeUninstallFailure(reason).code);
    } finally {
      setUninstallingSlug(null);
    }
  }

  return (
    <>
      <section className="library-page" aria-labelledby="library-title">
        {uninstallSuccess && (
          <p className="library-notice" role="status">
            {uninstallSuccess}
          </p>
        )}
        <header className="library-header">
          <div>
            <span className="eyebrow">{t('library.eyebrow')}</span>
            <h1 id="library-title">{t('library.title')}</h1>
            <p>{t('library.intro')}</p>
          </div>
          <div className="library-tools">
            <input
              aria-label={t('library.searchLabel')}
              placeholder={t('library.searchPlaceholder')}
              value={query}
              onChange={(event) => setQuery(event.target.value)}
            />
            <button onClick={() => setVersion((value) => value + 1)}>
              {t('common.refresh')}
            </button>
          </div>
        </header>

        {loading ? (
          <div
            className="library-grid"
            aria-label={t('library.loading')}
            role="status"
          >
            {[1, 2, 3].map((item) => (
              <div className="library-card skeleton" key={item} />
            ))}
          </div>
        ) : error ? (
          <div className="catalog-state error-state" role="alert">
            <span>{t('library.loadError')}</span>
            <strong>{error}</strong>
            <button onClick={() => setVersion((value) => value + 1)}>
              {t('common.retry')}
            </button>
          </div>
        ) : catalog?.games.length === 0 ? (
          <div className="catalog-state library-empty">
            <strong>{t('library.emptyTitle')}</strong>
            <span>{t('library.emptyText')}</span>
            <Link to="/">{t('library.explore')}</Link>
          </div>
        ) : games.length === 0 ? (
          <div className="catalog-state">
            <strong>{t('library.noResults', { query })}</strong>
            <button onClick={() => setQuery('')}>
              {t('common.clearSearch')}
            </button>
          </div>
        ) : (
          <>
            <div className="library-summary">
              <span>{t('common.games', { count: catalog?.total ?? 0 })}</span>
              <span>{t('library.signedIn', { email: user.email })}</span>
            </div>
            <div className="library-grid">
              {games.map((game) => {
                const date = readableDate(game.acquiredAt, i18n.language);
                const job = progress[game.slug];
                const isBusy = Boolean(
                  job &&
                  [
                    'queued',
                    'resolving',
                    'downloading',
                    'verifying',
                    'extracting',
                    'installing',
                  ].includes(job.phase),
                );
                const installation = installed[game.slug];
                const needsRepair = installation?.status === 'REPAIR_NEEDED';
                const updateVersion = availableUpdates[game.slug];
                const isLaunching = launchingSlug === game.slug;
                const isPlaying = Boolean(playing[game.slug]);
                const progressLabel = job ? t(`downloads.${job.phase}`) : null;
                const installFeedbackId = `install-feedback-${game.slug}`;
                const launchFeedbackId = `launch-feedback-${game.slug}`;
                return (
                  <article className="library-card" key={game.libraryId}>
                    <div className="library-art">
                      {game.bannerUrl || game.iconUrl ? (
                        <img
                          alt={t('library.artwork', { title: game.title })}
                          src={game.bannerUrl ?? game.iconUrl ?? ''}
                        />
                      ) : (
                        <span>{game.title.slice(0, 1)}</span>
                      )}
                      <span className="owned-badge">{t('library.owned')}</span>
                    </div>
                    <div className="library-card-content">
                      <div>
                        <h2>{game.title}</h2>
                        <p>{game.developerName}</p>
                      </div>
                      <div className="ownership-row">
                        {game.outlet?.logoUrl ? (
                          <img alt="" src={game.outlet.logoUrl} />
                        ) : (
                          <span className="outlet-mark" aria-hidden="true">
                            ◉
                          </span>
                        )}
                        <div>
                          <strong>
                            {game.outlet
                              ? t('library.acquiredVia', {
                                  outlet: game.outlet.name,
                                })
                              : game.acquisitionType === 'GRANT'
                                ? t('library.granted')
                                : game.acquisitionType === 'MANIFOLD_STORE'
                                  ? t('library.acquiredStore')
                                  : t('library.acquiredManifold')}
                          </strong>
                          {date && <span>{date}</span>}
                        </div>
                      </div>
                      <div className="library-availability">
                        <span>{t('library.access')}</span>
                        {isBusy && job ? (
                          <strong>
                            {Math.round(
                              (job.downloadedBytes /
                                Math.max(job.totalBytes, 1)) *
                                100,
                            )}
                            %
                          </strong>
                        ) : needsRepair ? (
                          <strong>{t('library.repairNeeded')}</strong>
                        ) : installation ? (
                          <strong>v{installation.version}</strong>
                        ) : null}
                      </div>
                      {isBusy && job && (
                        <div
                          className="install-progress"
                          aria-label={t('library.progressLabel', {
                            title: game.title,
                            phase: progressLabel,
                          })}
                          aria-valuemax={100}
                          aria-valuemin={0}
                          aria-valuenow={Math.round(
                            (job.downloadedBytes /
                              Math.max(job.totalBytes, 1)) *
                              100,
                          )}
                          role="progressbar"
                        >
                          <span
                            style={{
                              width: `${Math.round((job.downloadedBytes / Math.max(job.totalBytes, 1)) * 100)}%`,
                            }}
                          />
                        </div>
                      )}
                      <button
                        className="game-action"
                        aria-describedby={
                          [
                            job?.phase === 'failed' ? installFeedbackId : null,
                            launchErrors[game.slug] ? launchFeedbackId : null,
                          ]
                            .filter(Boolean)
                            .join(' ') || undefined
                        }
                        disabled={isBusy || isLaunching || isPlaying}
                        onClick={() => void handleGameAction(game)}
                      >
                        {isBusy
                          ? progressLabel
                          : isPlaying
                            ? t('library.playing')
                            : isLaunching
                              ? t('library.launching')
                              : needsRepair
                                ? t('library.repair')
                                : updateVersion
                                  ? t('library.update')
                                  : installation
                                    ? t('library.play')
                                    : job?.phase === 'failed'
                                      ? t('library.retryInstall')
                                      : t('library.install')}
                      </button>
                      {installation && (
                        <button
                          className="uninstall-action"
                          disabled={isBusy || uninstallingSlug === game.slug}
                          onClick={() => {
                            setUninstallError(null);
                            setUninstallSuccess(null);
                            setUninstallTarget(game);
                          }}
                        >
                          {uninstallingSlug === game.slug
                            ? t('library.uninstalling')
                            : t('library.uninstall')}
                        </button>
                      )}
                      {job?.phase === 'failed' && job.error && (
                        <p
                          className="install-error"
                          id={installFeedbackId}
                          role="alert"
                        >
                          {t(distributionErrorKey(job.errorCode))}{' '}
                          {t(
                            job.retryable === false
                              ? 'downloads.nonRetryableHelp'
                              : 'downloads.failedHelp',
                          )}
                        </p>
                      )}
                      {launchErrors[game.slug] && (
                        <p
                          className="install-error"
                          id={launchFeedbackId}
                          role="alert"
                        >
                          {t('errors.launchFailed')}
                        </p>
                      )}
                    </div>
                  </article>
                );
              })}
            </div>
          </>
        )}
      </section>
      {uninstallTarget && (
        <div className="dialog-backdrop">
          <div
            aria-describedby="uninstall-description"
            aria-labelledby="uninstall-title"
            aria-modal="true"
            className="confirmation-dialog"
            ref={uninstallDialogRef}
            role="dialog"
          >
            <span className="eyebrow">{t('library.uninstallEyebrow')}</span>
            <h2 id="uninstall-title">
              {t('library.uninstallTitle', { title: uninstallTarget.title })}
            </h2>
            <p id="uninstall-description">
              {t('library.uninstallDescription')}
            </p>
            <p className="confirmation-note">{t('library.uninstallKeeps')}</p>
            {uninstallError && (
              <p className="install-error" role="alert">
                {t(uninstallErrorKey(uninstallError))}
              </p>
            )}
            <div className="dialog-actions">
              <button
                className="secondary-action"
                disabled={Boolean(uninstallingSlug)}
                onClick={() => {
                  setUninstallTarget(null);
                  setUninstallError(null);
                }}
                ref={cancelUninstallRef}
              >
                {t('common.cancel')}
              </button>
              <button
                className="danger-action"
                disabled={Boolean(uninstallingSlug)}
                onClick={() => void confirmUninstall()}
              >
                {uninstallingSlug
                  ? t('library.uninstalling')
                  : t('library.confirmUninstall')}
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
}
