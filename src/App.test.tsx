import { invoke } from '@tauri-apps/api/core';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import App from './App';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

const catalog = {
  games: [
    {
      id: 'game-1',
      slug: 'capyvarias',
      title: 'Capyvarias',
      description: 'A cozy post-apocalyptic colony game.',
      price: '4.49',
      developerName: 'Piebox',
      tags: ['Cozy', 'Management'],
      bannerUrl: 'https://example.com/banner.jpg',
      iconUrl: null,
      status: 'ACTIVE',
      ownershipStatus: 'CLAIMED',
      purchaseMode: 'PLATFORM',
      externalOffer: null,
      displayPrice: {
        amount: '20.21',
        baseAmount: null,
        currency: 'BRL',
        symbol: 'R$',
      },
      discountLabel: null,
      reviewScore: 'MIXED',
    },
  ],
  pagination: { page: 1, limit: 12, total: 1, pages: 1 },
  currency: 'BRL',
};

afterEach(cleanup);

beforeEach(() => {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === 'list_store_games') return Promise.resolve(catalog);
    if (command === 'current_user') return Promise.resolve(null);
    return Promise.resolve({});
  });
});

it('renders primary desktop navigation', () => {
  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );
  expect(screen.getByRole('link', { name: 'Store' })).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'Downloads' })).toBeInTheDocument();
  expect(
    screen.queryByRole('link', { name: 'Studio' }),
  ).not.toBeInTheDocument();
});

it('renders games returned by the native catalog command', async () => {
  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );
  expect(
    await screen.findByRole('heading', { name: 'Capyvarias' }),
  ).toBeInTheDocument();
  expect(screen.getByText('R$ 20.21')).toBeInTheDocument();
  expect(invoke).toHaveBeenCalledWith('list_store_games', {
    query: null,
  });
});

it('reveals Studio only after an authenticated publisher has a studio', async () => {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === 'current_user') {
      return Promise.resolve({
        id: 'user-1',
        username: 'publisher',
        email: 'publisher@example.com',
      });
    }
    if (command === 'list_publisher_studios') {
      return Promise.resolve([
        {
          id: 'studio-1',
          slug: 'studio',
          name: 'Studio',
          description: null,
          logoUrl: null,
          isPublisher: true,
          ownerId: 'user-1',
        },
      ]);
    }
    if (command === 'list_store_games') return Promise.resolve(catalog);
    return Promise.resolve({});
  });

  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );

  expect(
    await screen.findByRole('link', { name: 'Studio' }),
  ).toBeInTheDocument();
});

it('renders a Steam-only promotion without presenting it as free', async () => {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === 'list_store_games') {
      return Promise.resolve({
        ...catalog,
        games: [
          {
            ...catalog.games[0],
            id: 'steam-game',
            slug: 'steam-game',
            title: 'Steam Game',
            price: null,
            displayPrice: null,
            status: 'ONLY_DISPLAY',
            ownershipStatus: 'UNCLAIMED',
            purchaseMode: 'STEAM_ONLY',
            externalOffer: {
              amount: '13.39',
              originalAmount: '19.99',
              currency: 'USD',
              discountPercent: 33,
              url: 'https://store.steampowered.com/app/400',
              capturedAt: '2026-08-20T12:00:00.000Z',
            },
          },
        ],
      });
    }
    if (command === 'current_user') return Promise.resolve(null);
    return Promise.resolve({});
  });

  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );

  expect(await screen.findByText('-33%')).toBeInTheDocument();
  expect(screen.getByText('$13.39')).toBeInTheDocument();
  expect(screen.getByText('$19.99')).toBeInTheDocument();
  expect(screen.queryByText('Free')).not.toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'View on Steam' })).toHaveAttribute(
    'href',
    'https://store.steampowered.com/app/400',
  );
});

it('treats an absent Steam price as unavailable, never free', async () => {
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === 'list_store_games') {
      return Promise.resolve({
        ...catalog,
        games: [
          {
            ...catalog.games[0],
            id: 'steam-unpriced',
            slug: 'steam-unpriced',
            title: 'Unpriced Steam Game',
            price: null,
            displayPrice: null,
            status: 'ONLY_DISPLAY',
            ownershipStatus: 'UNCLAIMED',
            purchaseMode: 'STEAM_ONLY',
            externalOffer: {
              amount: null,
              originalAmount: null,
              currency: null,
              discountPercent: null,
              url: 'https://store.steampowered.com/app/401',
              capturedAt: '2026-08-20T12:00:00.000Z',
            },
          },
        ],
      });
    }
    if (command === 'current_user') return Promise.resolve(null);
    return Promise.resolve({});
  });

  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );

  expect(
    await screen.findByText('Steam price unavailable'),
  ).toBeInTheDocument();
  expect(screen.queryByText('Free')).not.toBeInTheDocument();
});
