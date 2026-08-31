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
import {
  isPublisherError,
  publisherAdapter,
  publisherFixturesEnabled,
  PublisherStudio,
} from './publishing';
import { ReleaseWizardRoute } from './release-wizard';
import { SettingsPage } from './settings';
import { StudioGamePage, StudioPage } from './studio';

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

type PurchaseMode = 'PLATFORM' | 'STEAM_ONLY' | 'UNAVAILABLE';

type ExternalOffer = {
  provider: 'STEAM';
  amount: string | null;
  originalAmount: string | null;
  discountPercent: number | null;
  currency: string | null;
  url: string;
  capturedAt: string | null;
};

type StoreGame = {
  id: string;
  slug: string;
  title: string;
  description: string;
  price: string | null;
  developerName: string;
  tags: string[];
  bannerUrl: string | null;
  iconUrl: string | null;
  displayPrice: StoreDisplayPrice | null;
  discountLabel: string | null;
  reviewScore: string | null;
  status: 'ACTIVE' | 'ONLY_DISPLAY' | 'INACTIVE' | 'PRIVATE';
  ownershipStatus: 'UNCLAIMED' | 'CLAIMED';
  purchaseMode: PurchaseMode;
  externalOffer: ExternalOffer | null;
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
  if (game.purchaseMode === 'STEAM_ONLY') {
    const offer = game.externalOffer;
    if (offer?.amount === null || offer?.amount === undefined) return null;
    if (!offer.currency) return offer.amount;
    try {
      return new Intl.NumberFormat(locale, {
        style: 'currency',
        currency: offer.currency,
      }).format(Number(offer.amount));
    } catch {
      return `${offer.currency} ${offer.amount}`;
    }
  }
  if (game.purchaseMode !== 'PLATFORM') return null;
  if (game.displayPrice) {
    if (Number(game.displayPrice.amount) === 0) return free;
    return `${game.displayPrice.symbol} ${game.displayPrice.amount}`;
  }
  if (game.price === null) return null;
  if (Number(game.price) === 0) return free;
  return new Intl.NumberFormat(locale, {
    style: 'currency',
    currency: fallbackCurrency,
  }).format(Number(game.price));
}

function formatExternalOriginalPrice(game: StoreGame, locale: string) {
  const offer = game.externalOffer;
  if (
    !offer?.originalAmount ||
    offer.amount === null ||
    offer.originalAmount === offer.amount ||
    !offer.discountPercent
  ) {
    return null;
  }
  if (!offer.currency) return offer.originalAmount;
  try {
    return new Intl.NumberFormat(locale, {
      style: 'currency',
      currency: offer.currency,
    }).format(Number(offer.originalAmount));
  } catch {
    return `${offer.currency} ${offer.originalAmount}`;
  }
}

function discountLabel(game: StoreGame) {
  if (game.purchaseMode === 'STEAM_ONLY') {
    const percent = game.externalOffer?.discountPercent;
    return percent && formatExternalOriginalPrice(game, 'en-US')
      ? `-${percent}%`
      : null;
  }
  return game.discountLabel;
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
      .catch(() => {
        if (active) setError(t('store.unavailable'));
      })
      .finally(() => {
        if (active) setLoading(false);
      });
    return () => {
      active = false;
    };
  }, [requestVersion, submittedQuery, t]);

  const featured = catalog?.games[0] ?? null;
  const featuredDiscountLabel = featured ? discountLabel(featured) : null;
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
                {featuredDiscountLabel && (
                  <span className="discount featured-discount">
                    {featuredDiscountLabel}
                  </span>
                )}
                <div className="featured-content">
                  <span className="pill">{t('store.featured')}</span>
                  <h2>{featured.title}</h2>
                  <p>{featured.description}</p>
                  <div className="featured-meta">
                    <strong className="price-stack">
                      {formatPrice(
                        featured,
                        catalog.currency,
                        i18n.language,
                        t('common.free'),
                      ) ||
                        (featured.purchaseMode === 'STEAM_ONLY'
                          ? t('store.steamPriceUnavailable')
                          : t('store.catalogOnly'))}
                      {formatExternalOriginalPrice(featured, i18n.language) && (
                        <span className="price-original">
                          {formatExternalOriginalPrice(featured, i18n.language)}
                        </span>
                      )}
                    </strong>
                    <span>{featured.developerName}</span>
                  </div>
                  {featured.purchaseMode === 'STEAM_ONLY' &&
                  featured.externalOffer ? (
                    <a
                      className="availability-label"
                      href={featured.externalOffer.url}
                      target="_blank"
                      rel="noreferrer"
                    >
                      {t('store.viewOnSteam')}
                    </a>
                  ) : (
                    <span className="availability-label">
                      {featured.purchaseMode === 'PLATFORM'
                        ? t('store.available')
                        : t('store.catalogOnly')}
                    </span>
                  )}
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
                {games.map((game) => {
                  const gameDiscountLabel = discountLabel(game);
                  const originalPrice = formatExternalOriginalPrice(
                    game,
                    i18n.language,
                  );
                  return (
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
                        {gameDiscountLabel && (
                          <span className="discount">{gameDiscountLabel}</span>
                        )}
                      </div>
                      <div className="game-card-body">
                        <div>
                          <h3>{game.title}</h3>
                          <p>{game.developerName}</p>
                        </div>
                        <div className="price-stack">
                          {originalPrice && (
                            <span className="price-original">
                              {originalPrice}
                            </span>
                          )}
                          <strong>
                            {formatPrice(
                              game,
                              catalog.currency,
                              i18n.language,
                              t('common.free'),
                            ) ||
                              (game.purchaseMode === 'STEAM_ONLY'
                                ? t('store.steamPriceUnavailable')
                                : t('store.catalogOnly'))}
                          </strong>
                          {game.purchaseMode === 'STEAM_ONLY' &&
                            game.externalOffer && (
                              <a
                                className="steam-link"
                                href={game.externalOffer.url}
                                target="_blank"
                                rel="noreferrer"
                              >
                                {t('store.viewOnSteam')}
                              </a>
                            )}
                        </div>
                      </div>
                      <div className="tag-row">
                        {game.tags.slice(0, 3).map((tag) => (
                          <span key={tag}>{tag}</span>
                        ))}
                      </div>
                    </article>
                  );
                })}
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
  const [studios, setStudios] = useState<PublisherStudio[]>([]);
  const [online, setOnline] = useState(navigator.onLine);

  useEffect(() => {
    if (publisherFixturesEnabled) {
      setUser({
        id: 'fixture-user',
        username: 'publisher',
        email: 'publisher@example.test',
      });
      setCheckingSession(false);
      return;
    }
    invoke<AuthUser | null>('current_user')
      .then(setUser)
      .catch(() => setUser(null))
      .finally(() => setCheckingSession(false));
  }, []);

  useEffect(() => {
    if (!user) {
      setStudios([]);
      return;
    }
    let active = true;
    publisherAdapter
      .listStudios()
      .then((values) => {
        if (active) setStudios(values);
      })
      .catch((error: unknown) => {
        if (!active) return;
        setStudios([]);
        if (
          isPublisherError(error) &&
          error.code === 'AUTHENTICATION_REQUIRED'
        ) {
          setUser(null);
        }
      });
    return () => {
      active = false;
    };
  }, [user]);

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

  const expireSession = useCallback(() => {
    setUser(null);
    setStudios([]);
  }, []);

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
    ...(studios.length > 0 ? [{ label: t('nav.studio'), to: '/studio' }] : []),
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
          <Route
            path="/studio"
            element={
              user && studios.length > 0 ? (
                <StudioPage studios={studios} onUnauthorized={expireSession} />
              ) : (
                <Navigate replace to={user ? '/library' : '/login'} />
              )
            }
          />
          <Route
            path="/studio/:studioSlug/games/:gameSlug"
            element={
              user && studios.length > 0 ? (
                <StudioGamePage
                  studios={studios}
                  onUnauthorized={expireSession}
                />
              ) : (
                <Navigate replace to={user ? '/library' : '/login'} />
              )
            }
          />
          <Route
            path="/studio/:studioSlug/games/:gameSlug/releases/new"
            element={
              user && studios.length > 0 ? (
                <ReleaseWizardRoute
                  studios={studios}
                  onUnauthorized={expireSession}
                />
              ) : (
                <Navigate replace to={user ? '/library' : '/login'} />
              )
            }
          />
          <Route
            path="/studio/:studioSlug/games/:gameSlug/releases/:releaseId"
            element={
              user && studios.length > 0 ? (
                <ReleaseWizardRoute
                  studios={studios}
                  onUnauthorized={expireSession}
                />
              ) : (
                <Navigate replace to={user ? '/library' : '/login'} />
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
