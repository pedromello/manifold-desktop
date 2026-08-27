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
