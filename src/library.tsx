import { invoke } from '@tauri-apps/api/core';
import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { AuthPanel, AuthUser } from './auth';
import { useInstallations } from './installation';

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
    progress,
  } = useInstallations();
  const [catalog, setCatalog] = useState<LibraryCatalog | null>(null);
  const [query, setQuery] = useState('');
  const [loading, setLoading] = useState(Boolean(user));
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState(0);

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
        const message =
          typeof reason === 'string' ? reason : t('library.unavailable');
        setError(message);
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

  return (
    <section className="library-page" aria-labelledby="library-title">
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
                  'installing',
                ].includes(job.phase),
              );
              const installation = installed[game.slug];
              const needsRepair = installation?.status === 'REPAIR_NEEDED';
              const updateVersion = availableUpdates[game.slug];
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
                                : game.acquisitionLabel}
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
                        aria-label={`${game.title} ${job.phase}`}
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
                      disabled={isBusy}
                      onClick={() =>
                        installation && !needsRepair && !updateVersion
                          ? void launch(game.slug)
                          : void install(game.slug, game.title)
                      }
                    >
                      {isBusy
                        ? t('library.installing')
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
                    {job?.phase === 'failed' && job.error && (
                      <p className="install-error" role="alert">
                        {job.error}
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
  );
}
