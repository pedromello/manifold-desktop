import { invoke } from '@tauri-apps/api/core';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { LibraryPage } from './library';
import { InstallationProvider } from './installation';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

afterEach(cleanup);

it('shows owned games with their acquisition outlet', async () => {
  vi.mocked(invoke).mockResolvedValue({
    total: 1,
    games: [
      {
        libraryId: 'library-1',
        id: 'game-1',
        slug: 'capyvarias',
        title: 'Capyvarias',
        description: 'Cozy colony game',
        developerName: 'Piebox',
        bannerUrl: null,
        iconUrl: null,
        acquiredAt: '2026-08-12T12:00:00.000Z',
        outlet: {
          id: 'outlet-1',
          slug: 'cozy-outlet',
          name: 'Cozy Outlet',
          logoUrl: null,
        },
        acquisitionLabel: 'Acquired via Cozy Outlet',
      },
    ],
  });

  render(
    <MemoryRouter>
      <InstallationProvider>
        <LibraryPage
          user={{ id: 'user-1', username: 'pedro', email: 'pedro@example.com' }}
          onAuthenticated={vi.fn()}
          onSessionExpired={vi.fn()}
        />
      </InstallationProvider>
    </MemoryRouter>,
  );

  expect(
    await screen.findByRole('heading', { name: 'Capyvarias' }),
  ).toBeInTheDocument();
  expect(screen.getByText('Acquired via Cozy Outlet')).toBeInTheDocument();
  expect(screen.getByText('Access confirmed')).toBeInTheDocument();
  expect(invoke).toHaveBeenCalledWith('list_library');
});
