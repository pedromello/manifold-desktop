import { FormEvent, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate, useParams } from 'react-router-dom';
import {
  isPublisherError,
  findStoredPublisherRelease,
  publisherAdapter,
  PublisherAdapter,
  PublisherGame,
  PublisherRelease,
  PublisherStudio,
  storePublisherRelease,
} from './publishing';

type StudioPageProps = {
  studios: PublisherStudio[];
  adapter?: PublisherAdapter;
  onUnauthorized: () => void;
};

function publisherErrorKey(error: unknown) {
  if (!isPublisherError(error)) return 'studio.errors.unavailable';
  if (error.code === 'AUTHENTICATION_REQUIRED')
    return 'studio.errors.authentication';
  if (error.code === 'FORBIDDEN' || error.code === 'PERMISSION_DENIED')
    return 'studio.errors.permission';
  if (error.code === 'NOT_FOUND') return 'studio.errors.notFound';
  return 'studio.errors.unavailable';
}

function useStudioGames(
  studioSlug: string | undefined,
  adapter: PublisherAdapter,
  onUnauthorized: () => void,
) {
  const [games, setGames] = useState<PublisherGame[]>([]);
  const [loading, setLoading] = useState(Boolean(studioSlug));
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [requestVersion, setRequestVersion] = useState(0);

  useEffect(() => {
    if (!studioSlug) {
      setGames([]);
      setLoading(false);
      return;
    }
    let active = true;
    setLoading(true);
    setErrorKey(null);
    adapter
      .listGames(studioSlug)
      .then((value) => {
        if (active) setGames(value);
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (
          isPublisherError(error) &&
          error.code === 'AUTHENTICATION_REQUIRED'
        ) {
          onUnauthorized();
        }
        setErrorKey(publisherErrorKey(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [adapter, onUnauthorized, requestVersion, studioSlug]);

  return {
    games,
    loading,
    errorKey,
    retry: () => setRequestVersion((value) => value + 1),
  };
}

const RELEASES_PER_PAGE = 20;

function useGameReleases(
  gameSlug: string | undefined,
  adapter: PublisherAdapter,
  onUnauthorized: () => void,
) {
  const [releases, setReleases] = useState<PublisherRelease[]>([]);
  const [pagination, setPagination] = useState({
    page: 1,
    limit: RELEASES_PER_PAGE,
    total: 0,
    pages: 0,
  });
  const [page, setPage] = useState(1);
  const [loading, setLoading] = useState(Boolean(gameSlug));
  const [errorKey, setErrorKey] = useState<string | null>(null);
  const [requestVersion, setRequestVersion] = useState(0);

  useEffect(() => {
    setPage(1);
  }, [gameSlug]);

  useEffect(() => {
    if (!gameSlug) {
      setReleases([]);
      setLoading(false);
      return;
    }
    let active = true;
    setLoading(true);
    setErrorKey(null);
    adapter
      .listReleases(gameSlug, page, RELEASES_PER_PAGE)
      .then((value) => {
        if (!active) return;
        setReleases(value.releases);
        setPagination(value.pagination);
      })
      .catch((error: unknown) => {
        if (!active) return;
        if (
          isPublisherError(error) &&
          error.code === 'AUTHENTICATION_REQUIRED'
        ) {
          onUnauthorized();
        }
        setErrorKey(publisherErrorKey(error));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [adapter, gameSlug, onUnauthorized, page, requestVersion]);

  return {
    releases,
    pagination,
    loading,
    errorKey,
    previous: () => setPage((value) => Math.max(1, value - 1)),
    next: () => setPage((value) => Math.min(pagination.pages || 1, value + 1)),
    retry: () => setRequestVersion((value) => value + 1),
  };
}
export function StudioPage({
  studios,
  adapter = publisherAdapter,
  onUnauthorized,
}: StudioPageProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [selectedSlug, setSelectedSlug] = useState(studios[0]?.slug ?? '');
  const { games, loading, errorKey, retry } = useStudioGames(
    selectedSlug,
    adapter,
    onUnauthorized,
  );
  const studio = studios.find((value) => value.slug === selectedSlug);

  useEffect(() => {
    if (!studios.some((value) => value.slug === selectedSlug)) {
      setSelectedSlug(studios[0]?.slug ?? '');
    }
  }, [selectedSlug, studios]);

  return (
    <section className="studio-page" aria-labelledby="studio-title">
      <header className="page-header studio-header">
        <div>
          <span className="eyebrow">{t('studio.eyebrow')}</span>
          <h1 id="studio-title">{t('studio.title')}</h1>
          <p>{t('studio.intro')}</p>
        </div>
        {studios.length > 1 && (
          <label className="studio-selector">
            <span>{t('studio.chooseStudio')}</span>
            <select
              value={selectedSlug}
              onChange={(event) => setSelectedSlug(event.target.value)}
            >
              {studios.map((value) => (
                <option key={value.id} value={value.slug}>
                  {value.name}
                </option>
              ))}
            </select>
          </label>
        )}
      </header>

      {studio && (
        <div className="studio-context">
          <div className="studio-identity">
            <strong>{studio.name}</strong>
            <span>{studio.description || t('studio.noDescription')}</span>
          </div>
          {studio.isPublisher && (
            <span className="studio-access">{t('studio.publisher')}</span>
          )}
        </div>
      )}

      {loading ? (
        <div className="catalog-state" role="status">
          <span className="spinner" />
          <strong>{t('studio.loadingGames')}</strong>
        </div>
      ) : errorKey ? (
        <div className="catalog-state error-state" role="alert">
          <span>{t('studio.loadError')}</span>
          <strong>{t(errorKey)}</strong>
          <button onClick={retry}>{t('common.retry')}</button>
        </div>
      ) : games.length === 0 ? (
        <div className="catalog-state">
          <strong>{t('studio.emptyTitle')}</strong>
          <span>{t('studio.emptyHelp')}</span>
        </div>
      ) : (
        <div className="studio-game-list">
          {games.map((game) => (
            <article className="studio-game-row" key={game.id}>
              <div className="studio-game-art">
                {game.bannerUrl || game.iconUrl ? (
                  <img src={game.bannerUrl ?? game.iconUrl ?? ''} alt="" />
                ) : (
                  <span>{game.title.slice(0, 1).toUpperCase()}</span>
                )}
              </div>
              <div className="studio-game-copy">
                <h2>{game.title}</h2>
                <p>{game.description}</p>
              </div>
              <span className="game-state">
                {t('studio.gameStatus.' + game.status.toLowerCase(), {
                  defaultValue: game.status,
                })}
              </span>
              <button
                className="row-action"
                onClick={() =>
                  navigate(`/studio/${selectedSlug}/games/${game.slug}`)
                }
              >
                {t('studio.manageGame')} <span aria-hidden="true">→</span>
              </button>
            </article>
          ))}
        </div>
      )}
    </section>
  );
}

export function StudioGamePage({
  studios,
  adapter = publisherAdapter,
  onUnauthorized,
}: StudioPageProps) {
  const { t, i18n } = useTranslation();
  const { studioSlug, gameSlug } = useParams();
  const { games, loading, errorKey, retry } = useStudioGames(
    studioSlug,
    adapter,
    onUnauthorized,
  );
  const {
    releases,
    pagination,
    loading: releasesLoading,
    errorKey: releasesErrorKey,
    previous,
    next,
    retry: retryReleases,
  } = useGameReleases(gameSlug, adapter, onUnauthorized);
  const game = games.find((value) => value.slug === gameSlug);
  const studio = studios.find((value) => value.slug === studioSlug);

  function rememberRelease(release: PublisherRelease) {
    if (!studioSlug || !gameSlug || !game) return;
    const existing = findStoredPublisherRelease(release.id);
    if (existing) {
      storePublisherRelease({ ...existing, release });
      return;
    }
    storePublisherRelease({
      studioSlug,
      gameSlug,
      gameTitle: game.title,
      release,
      archivePath: null,
      inspection: null,
      manifest: null,
      phase: 'file',
      uploadStarted: false,
      updatedAt: new Date().toISOString(),
    });
  }

  if (loading || releasesLoading) {
    return (
      <section className="studio-page">
        <div className="catalog-state" role="status">
          <span className="spinner" />
          <strong>{t('studio.loadingGame')}</strong>
        </div>
      </section>
    );
  }

  if (errorKey || releasesErrorKey || !game || !studio) {
    return (
      <section className="studio-page">
        <div className="catalog-state error-state" role="alert">
          <span>{t('studio.gameLoadError')}</span>
          <strong>
            {t(errorKey ?? releasesErrorKey ?? 'studio.errors.notFound')}
          </strong>
          <button
            onClick={() => {
              retry();
              retryReleases();
            }}
          >
            {t('common.retry')}
          </button>
        </div>
      </section>
    );
  }

  return (
    <section className="studio-page" aria-labelledby="studio-game-title">
      <Link className="back-link" to="/studio">
        ← {studio.name}
      </Link>
      <header className="page-header game-release-header">
        <div>
          <span className="eyebrow">{t('studio.gameEyebrow')}</span>
          <h1 id="studio-game-title">{game.title}</h1>
          <p>{t('studio.releasesIntro')}</p>
        </div>
        <Link
          className="primary-link"
          to={`/studio/${studio.slug}/games/${game.slug}/releases/new`}
        >
          {t('studio.newRelease')}
        </Link>
      </header>

      {releases.length === 0 ? (
        <div className="catalog-state">
          <strong>{t('studio.noReleases')}</strong>
          <span>{t('studio.noReleasesHelp')}</span>
        </div>
      ) : (
        <>
          <div className="release-list">
            {releases.map((release) => (
              <article className="release-card" key={release.id}>
                <div className="release-copy">
                  <h2>{release.version}</h2>
                  <span>
                    {t('studio.releaseNumber', {
                      number: release.releaseNumber,
                    })}
                    {' · '}
                    {new Intl.DateTimeFormat(i18n.language, {
                      dateStyle: 'medium',
                    }).format(new Date(release.createdAt))}
                  </span>
                </div>
                <span
                  className={`release-status status-${release.status.toLowerCase()}`}
                >
                  {t(`studio.status.${release.status.toLowerCase()}`, {
                    defaultValue: release.status,
                  })}
                </span>
                {release.status !== 'PUBLISHED' && (
                  <Link
                    className="secondary-link"
                    onClick={() => rememberRelease(release)}
                    to={`/studio/${studio.slug}/games/${game.slug}/releases/${release.id}`}
                  >
                    {t('studio.resume')}
                  </Link>
                )}
              </article>
            ))}
          </div>
          {pagination.pages > 1 && (
            <nav
              className="release-pagination"
              aria-label={t('studio.paginationLabel')}
            >
              <button
                className="secondary-action"
                disabled={pagination.page <= 1}
                onClick={previous}
                type="button"
              >
                {t('studio.previousPage')}
              </button>
              <span>
                {t('studio.pageCount', {
                  current: pagination.page,
                  total: pagination.pages,
                })}
              </span>
              <button
                className="secondary-action"
                disabled={pagination.page >= pagination.pages}
                onClick={next}
                type="button"
              >
                {t('studio.nextPage')}
              </button>
            </nav>
          )}
        </>
      )}
    </section>
  );
}

export function ReleaseDataForm({
  game,
  studioSlug,
  adapter = publisherAdapter,
}: {
  game: PublisherGame;
  studioSlug: string;
  adapter?: PublisherAdapter;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [version, setVersion] = useState('');
  const [notes, setNotes] = useState('');
  const [creating, setCreating] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);

  async function create(event: FormEvent) {
    event.preventDefault();
    setCreating(true);
    setErrorKey(null);
    try {
      const release = await adapter.createDraft(
        game.slug,
        version.trim(),
        notes.trim() || null,
      );
      const stored = {
        studioSlug,
        gameSlug: game.slug,
        gameTitle: game.title,
        release,
        archivePath: null,
        inspection: null,
        manifest: null,
        phase: 'file' as const,
        uploadStarted: false,
        updatedAt: new Date().toISOString(),
      };
      storePublisherRelease(stored);
      navigate(
        `/studio/${studioSlug}/games/${game.slug}/releases/${release.id}`,
        { replace: true },
      );
    } catch (error) {
      setErrorKey(publisherErrorKey(error));
    } finally {
      setCreating(false);
    }
  }

  return (
    <form className="release-form" onSubmit={create}>
      <label>
        <strong>{t('publisher.version')}</strong>
        <span>{t('publisher.versionHelp')}</span>
        <input
          aria-label={t('publisher.version')}
          autoFocus
          maxLength={50}
          required
          value={version}
          onChange={(event) => setVersion(event.target.value)}
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
          value={notes}
          onChange={(event) => setNotes(event.target.value)}
        />
      </label>
      <div className="wizard-actions">
        <button className="game-action" disabled={creating} type="submit">
          {creating ? t('publisher.creatingDraft') : t('publisher.createDraft')}
        </button>
      </div>
      {errorKey && (
        <p className="form-notice error" role="alert">
          {t(errorKey)}
        </p>
      )}
    </form>
  );
}
