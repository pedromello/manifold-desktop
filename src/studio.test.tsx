import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { PublisherAdapter, PublisherStudio } from './publishing';
import { StudioGamePage, StudioPage } from './studio';

const studios: PublisherStudio[] = [
  {
    id: 'studio-1',
    slug: 'studio-one',
    name: 'Studio One',
    description: 'First studio',
    logoUrl: null,
    isPublisher: true,
    ownerId: 'user-1',
  },
  {
    id: 'studio-2',
    slug: 'studio-two',
    name: 'Studio Two',
    description: null,
    logoUrl: null,
    isPublisher: false,
    ownerId: 'user-2',
  },
];

function adapter(listGames: PublisherAdapter['listGames']): PublisherAdapter {
  return {
    listStudios: vi.fn().mockResolvedValue(studios),
    listGames,
    listReleases: vi.fn().mockResolvedValue({
      releases: [],
      pagination: { page: 1, limit: 20, total: 0, pages: 0 },
    }),
    createDraft: vi.fn(),
    updateDraft: vi.fn(),
    selectArchive: vi.fn(),
    inspectArchive: vi.fn(),
    publish: vi.fn(),
    cancel: vi.fn(),
  };
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

it('shows a studio selector and its games without asking for technical ids', async () => {
  render(
    <MemoryRouter>
      <StudioPage
        studios={studios}
        adapter={adapter(
          vi.fn().mockResolvedValue([
            {
              id: 'game-1',
              slug: 'friendly-slug',
              title: 'A Friendly Game',
              description: 'Game description',
              status: 'PRIVATE',
              bannerUrl: null,
              iconUrl: null,
            },
          ]),
        )}
        onUnauthorized={vi.fn()}
      />
    </MemoryRouter>,
  );

  expect(screen.getByRole('combobox', { name: 'Studio' })).toBeInTheDocument();
  expect(
    await screen.findByRole('heading', { name: 'A Friendly Game' }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole('button', { name: 'View versions' }),
  ).toBeInTheDocument();
  expect(screen.queryByText('game-1')).not.toBeInTheDocument();
});

it('maps a scoped 403 to a useful permission state', async () => {
  render(
    <MemoryRouter>
      <StudioPage
        studios={[studios[0]]}
        adapter={adapter(
          vi.fn().mockRejectedValue({
            code: 'PERMISSION_DENIED',
            message: 'Forbidden',
            retryable: false,
          }),
        )}
        onUnauthorized={vi.fn()}
      />
    </MemoryRouter>,
  );

  expect(
    await screen.findByText('Your account cannot access this studio area.'),
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
});
it('lists backend releases and makes a remote draft resumable', async () => {
  const publishing = adapter(
    vi.fn().mockResolvedValue([
      {
        id: 'game-1',
        slug: 'friendly-slug',
        title: 'A Friendly Game',
        description: 'Game description',
        status: 'ACTIVE',
        bannerUrl: null,
        iconUrl: null,
      },
    ]),
  );
  publishing.listReleases = vi.fn().mockResolvedValue({
    releases: [
      {
        id: 'release-draft',
        gameId: 'game-1',
        version: '2.0.0',
        releaseNumber: 2,
        status: 'DRAFT',
        releaseNotes: 'Work in progress',
        publishedAt: null,
        createdAt: '2026-08-27T12:00:00.000Z',
        updatedAt: '2026-08-27T12:00:00.000Z',
        artifacts: [],
      },
      {
        id: 'release-published',
        gameId: 'game-1',
        version: '1.0.0',
        releaseNumber: 1,
        status: 'PUBLISHED',
        releaseNotes: null,
        publishedAt: '2026-08-26T12:00:00.000Z',
        createdAt: '2026-08-26T11:00:00.000Z',
        updatedAt: '2026-08-26T12:00:00.000Z',
        artifacts: [],
      },
    ],
    pagination: { page: 1, limit: 20, total: 2, pages: 1 },
  });

  render(
    <MemoryRouter initialEntries={['/studio/studio-one/games/friendly-slug']}>
      <Routes>
        <Route
          path="/studio/:studioSlug/games/:gameSlug"
          element={
            <StudioGamePage
              studios={[studios[0]]}
              adapter={publishing}
              onUnauthorized={vi.fn()}
            />
          }
        />
        <Route
          path="/studio/:studioSlug/games/:gameSlug/releases/:releaseId"
          element={<div>Resume destination</div>}
        />
      </Routes>
    </MemoryRouter>,
  );

  expect(
    await screen.findByRole('heading', { name: '2.0.0' }),
  ).toBeInTheDocument();
  expect(screen.getByRole('heading', { name: '1.0.0' })).toBeInTheDocument();
  expect(publishing.listReleases).toHaveBeenCalledWith('friendly-slug', 1, 20);
  expect(screen.queryByText(/saved on this computer/i)).not.toBeInTheDocument();

  fireEvent.click(screen.getByRole('link', { name: 'Continue' }));
  expect(await screen.findByText('Resume destination')).toBeInTheDocument();
  expect(
    window.localStorage.getItem('manifold.publisher.releases.v1'),
  ).toContain('release-draft');
});

it('loads the next backend release page', async () => {
  const publishing = adapter(
    vi.fn().mockResolvedValue([
      {
        id: 'game-1',
        slug: 'friendly-slug',
        title: 'A Friendly Game',
        description: '',
        status: 'ACTIVE',
        bannerUrl: null,
        iconUrl: null,
      },
    ]),
  );
  publishing.listReleases = vi.fn().mockImplementation((_slug, page) =>
    Promise.resolve({
      releases: [
        {
          id: `release-${page}`,
          gameId: 'game-1',
          version: page === 1 ? '2.0.0' : '1.0.0',
          releaseNumber: page === 1 ? 2 : 1,
          status: 'PUBLISHED',
          releaseNotes: null,
          publishedAt: '2026-08-27T12:00:00.000Z',
          createdAt: '2026-08-27T12:00:00.000Z',
          updatedAt: '2026-08-27T12:00:00.000Z',
          artifacts: [],
        },
      ],
      pagination: { page, limit: 20, total: 21, pages: 2 },
    }),
  );

  render(
    <MemoryRouter initialEntries={['/studio/studio-one/games/friendly-slug']}>
      <Routes>
        <Route
          path="/studio/:studioSlug/games/:gameSlug"
          element={
            <StudioGamePage
              studios={[studios[0]]}
              adapter={publishing}
              onUnauthorized={vi.fn()}
            />
          }
        />
      </Routes>
    </MemoryRouter>,
  );

  expect(await screen.findByText('Page 1 of 2')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Next' }));
  expect(await screen.findByText('Page 2 of 2')).toBeInTheDocument();
  expect(publishing.listReleases).toHaveBeenLastCalledWith(
    'friendly-slug',
    2,
    20,
  );
});
it('maps release-list authentication errors and expires the session', async () => {
  const publishing = adapter(
    vi.fn().mockResolvedValue([
      {
        id: 'game-1',
        slug: 'friendly-slug',
        title: 'A Friendly Game',
        description: '',
        status: 'ACTIVE',
        bannerUrl: null,
        iconUrl: null,
      },
    ]),
  );
  publishing.listReleases = vi.fn().mockRejectedValue({
    code: 'AUTHENTICATION_REQUIRED',
    message: 'Session expired',
    retryable: false,
  });
  const onUnauthorized = vi.fn();

  render(
    <MemoryRouter initialEntries={['/studio/studio-one/games/friendly-slug']}>
      <Routes>
        <Route
          path="/studio/:studioSlug/games/:gameSlug"
          element={
            <StudioGamePage
              studios={[studios[0]]}
              adapter={publishing}
              onUnauthorized={onUnauthorized}
            />
          }
        />
      </Routes>
    </MemoryRouter>,
  );

  expect(
    await screen.findByText('Your session expired. Sign in again to continue.'),
  ).toBeInTheDocument();
  expect(onUnauthorized).toHaveBeenCalledTimes(1);
});
