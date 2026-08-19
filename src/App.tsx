import { invoke } from '@tauri-apps/api/core';
import { FormEvent, useCallback, useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Navigate, NavLink, Route, Routes } from 'react-router-dom';
import manifoldLogo from './assets/manifold-logo.png';
import { AuthPanel, AuthUser } from './auth';
import { appConfig } from './config';
import { DownloadsPage } from './downloads';
import { InstallationProvider } from './installation';
import { LibraryPage } from './library';
import { SettingsPage } from './settings';

type AppInfo = {
  version: string;
  environment: string;
  platform: string;
  architecture: string;
};

type StoreDisplayPrice = {
  amount: string;
  baseAmount: string | null;
  currency: string;
  symbol: string;
};

type StoreGame = {
  id: string;
  slug: string;
  title: string;
  description: string;
  price: string;
  developerName: string;
  tags: string[];
  bannerUrl: string | null;
  iconUrl: string | null;
  displayPrice: StoreDisplayPrice | null;
  discountLabel: string | null;
  reviewScore: string | null;
};

type StoreCatalog = {
  games: StoreGame[];
  pagination: { page: number; limit: number; total: number; pages: number };
  currency: string;
};

function formatPrice(
  game: StoreGame,
  fallbackCurrency: string,
  locale: string,
  free: string,
) {
  if (game.displayPrice) {
    if (Number(game.displayPrice.amount) === 0) return free;
    return `${game.displayPrice.symbol} ${game.displayPrice.amount}`;
  }
  if (Number(game.price) === 0) return free;
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: fallbackCurrency,
  }).format(Number(game.price));
}

function StorePage() {
  const { t, i18n } = useTranslation();
  const [catalog, setCatalog] = useState<StoreCatalog | null>(null);
  const [query, setQuery] = useState('');
  const [submittedQuery, setSubmittedQuery] = useState('');
  const [requestVersion, setRequestVersion] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    setLoading(true);
    setError(null);
    invoke<StoreCatalog>('list_store_games', {
      query: submittedQuery || null,
    })
      .then((data) => {
        if (active) setCatalog(data);
      })
      .catch((reason: unknown) => {
        if (active)
          setError(
            typeof reason === 'string' ? reason : t('store.unavailable'),
          );
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [requestVersion, submittedQuery, t]);

  const featured = catalog?.games[0] ?? null;
  const games = useMemo(
    () => (featured ? (catalog?.games.slice(1) ?? []) : (catalog?.games ?? [])),
    [catalog, featured],
  );

  function submitSearch(event: FormEvent) {
    event.preventDefault();
    setSubmittedQuery(query.trim());
  }

  return (
    <div className="store-page">
      <header className="store-header">
        <div>
          <span className="eyebrow">{t('store.eyebrow')}</span>
          <h1>{t('store.title')}</h1>
        </div>
        <form className="store-search" onSubmit={submitSearch} role="search">
          <input
            aria-label={t('store.searchLabel')}
            maxLength={80}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t('store.searchPlaceholder')}
            value={query}
          />
          <button type="submit">{t('common.search')}</button>
        </form>
      </header>

      {loading && !catalog ? (
        <div className="catalog-state" role="status">
          <span className="spinner" />
          <strong>{t('store.loading')}</strong>
        </div>
      ) : error ? (
        <div className="catalog-state error-state" role="alert">
          <span>{t('store.loadError')}</span>
          <strong>{error}</strong>
          <button onClick={() => setRequestVersion((value) => value + 1)}>
            {t('common.retry')}
          </button>
        </div>
      ) : catalog && catalog.games.length === 0 ? (
        <div className="catalog-state">
          <strong>{t('store.noResults', { query: submittedQuery })}</strong>
          <button
            onClick={() => {
              setQuery('');
              setSubmittedQuery('');
            }}
          >
            {t('common.clearSearch')}
          </button>
        </div>
      ) : (
        catalog && (
          <>
            {featured && (
              <section
                className="featured-game"
                aria-label={t('store.featuredLabel')}
              >
                {featured.bannerUrl && (
                  <img src={featured.bannerUrl} alt="" aria-hidden="true" />
                )}
                <div className="featured-overlay" />
                <div className="featured-content">
                  <span className="pill">{t('store.featured')}</span>
                  <h2>{featured.title}</h2>
                  <p>{featured.description}</p>
                  <div className="featured-meta">
                    <strong>
                      {formatPrice(
                        featured,
                        catalog.currency,
                        i18n.language,
                        t('common.free'),
                      )}
                    </strong>
                    <span>{featured.developerName}</span>
                  </div>
                  <span className="availability-label">
                    {t('store.available')}
                  </span>
                </div>
              </section>
            )}

            <section className="catalog-section">
              <div className="section-heading">
                <div>
                  <span className="eyebrow">{t('store.discover')}</span>
                  <h2>
                    {submittedQuery
                      ? t('store.results', { query: submittedQuery })
                      : t('store.new')}
                  </h2>
                </div>
                <span>
                  {t('common.games', { count: catalog.pagination.total })}
                </span>
              </div>
              <div className="game-grid">
                {games.map((game) => (
                  <article className="game-card" key={game.id}>
                    <div className="game-art">
                      {game.bannerUrl || game.iconUrl ? (
                        <img
                          src={game.bannerUrl ?? game.iconUrl ?? ''}
                          alt=""
                        />
                      ) : (
                        <span>{game.title.slice(0, 1)}</span>
                      )}
                      {game.discountLabel && (
                        <span className="discount">{game.discountLabel}</span>
                      )}
                    </div>
                    <div className="game-card-body">
                      <div>
                        <h3>{game.title}</h3>
                        <p>{game.developerName}</p>
                      </div>
                      <strong>
                        {formatPrice(
                          game,
                          catalog.currency,
                          i18n.language,
                          t('common.free'),
                        )}
                      </strong>
                    </div>
                    <div className="tag-row">
                      {game.tags.slice(0, 3).map((tag) => (
                        <span key={tag}>{tag}</span>
                      ))}
                    </div>
                  </article>
                ))}
              </div>
            </section>
          </>
        )
      )}
    </div>
  );
}

export function ApplicationInfo() {
  const { t } = useTranslation();
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    invoke<AppInfo>('application_info', { environment: appConfig.environment })
      .then(setInfo)
      .catch(() =>
        setInfo({
          version: 'web',
          environment: appConfig.environment,
          platform: navigator.platform,
          architecture: 'browser',
        }),
      );
  }, []);
  return (
    <section className="coming-soon">
      <span className="eyebrow">{t('about.eyebrow')}</span>
      <h1>Manifold Desktop</h1>
      {info && (
        <dl>
          <dt>{t('about.version')}</dt>
          <dd>{info.version}</dd>
          <dt>{t('about.environment')}</dt>
          <dd>{info.environment}</dd>
          <dt>{t('about.operatingSystem')}</dt>
          <dd>{info.platform}</dd>
          <dt>{t('about.architecture')}</dt>
          <dd>{info.architecture}</dd>
        </dl>
      )}
    </section>
  );
}

function AppShell() {
  const { t } = useTranslation();
  const [user, setUser] = useState<AuthUser | null>(null);
  const [checkingSession, setCheckingSession] = useState(true);
  const [online, setOnline] = useState(navigator.onLine);

  useEffect(() => {
    invoke<AuthUser | null>('current_user')
      .then(setUser)
      .catch(() => setUser(null))
      .finally(() => setCheckingSession(false));
  }, []);

  useEffect(() => {
    const connected = () => setOnline(true);
    const disconnected = () => setOnline(false);
    window.addEventListener('online', connected);
    window.addEventListener('offline', disconnected);
    return () => {
      window.removeEventListener('online', connected);
      window.removeEventListener('offline', disconnected);
    };
  }, []);

  const expireSession = useCallback(() => setUser(null), []);

  async function handleLogout() {
    try {
      await invoke('logout');
    } finally {
      setUser(null);
    }
  }

  const navigation = [
    { label: t('nav.store'), to: '/' },
    { label: t('nav.library'), to: '/library' },
    { label: t('nav.downloads'), to: '/downloads' },
  ];

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <NavLink className="brand" to="/" aria-label="Manifold Store">
          <img className="brand-logo" src={manifoldLogo} alt="" />
          <span>{t('common.manifold')}</span>
        </NavLink>
        <nav aria-label={t('nav.primary')}>
          {navigation.map((item) => (
            <NavLink key={item.to} to={item.to} end={item.to === '/'}>
              {item.label}
            </NavLink>
          ))}
        </nav>
        <div className="sidebar-footer">
          <NavLink to="/settings">{t('nav.settings')}</NavLink>
          <NavLink to="/about">{t('nav.about')}</NavLink>
          {checkingSession ? (
            <span className="account-status">{t('account.checking')}</span>
          ) : user ? (
            <div className="account-card">
              <span className="account-avatar" aria-hidden="true">
                {user.username.slice(0, 1).toUpperCase()}
              </span>
              <div>
                <strong>{user.username}</strong>
                <span>{user.email}</span>
              </div>
              <button onClick={handleLogout}>{t('account.signOut')}</button>
            </div>
          ) : (
            <NavLink className="account-link" to="/login">
              {t('account.signIn')}
            </NavLink>
          )}
        </div>
      </aside>
      <main className="content">
        {!online && (
          <div className="offline-banner" role="status">
            {t('errors.offline')}
          </div>
        )}
        <Routes>
          <Route path="/" element={<StorePage />} />
          <Route
            path="/library"
            element={
              <LibraryPage
                user={user}
                onAuthenticated={setUser}
                onSessionExpired={expireSession}
              />
            }
          />
          <Route
            path="/login"
            element={
              user ? (
                <Navigate replace to="/library" />
              ) : (
                <AuthPanel onAuthenticated={setUser} />
              )
            }
          />
          <Route
            path="/signup"
            element={
              user ? (
                <Navigate replace to="/library" />
              ) : (
                <AuthPanel initialMode="signup" onAuthenticated={setUser} />
              )
            }
          />
          <Route path="/downloads" element={<DownloadsPage />} />
          <Route path="/settings" element={<SettingsPage />} />
          <Route path="/about" element={<ApplicationInfo />} />
        </Routes>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <InstallationProvider>
      <AppShell />
    </InstallationProvider>
  );
}
