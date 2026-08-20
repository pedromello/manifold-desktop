import { invoke } from '@tauri-apps/api/core';
import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { LibraryPage } from './library';
import { InstallationProvider } from './installation';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
});

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

it('offers repair instead of play when reconciliation finds a missing file', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') {
      return [
        {
          gameSlug: 'capyvarias',
          title: 'Capyvarias',
          version: '1.0.0',
          releaseId: 'release-1',
          releaseNumber: 1,
          artifactId: 'artifact-1',
          installedSizeBytes: '2048',
          installDirectory: 'C:\\Games\\Capyvarias',
          entrypoint: 'Capyvarias.exe',
          installedAt: '2026-08-19T12:00:00.000Z',
          status: 'REPAIR_NEEDED',
        },
      ] as never;
    }
    if (command === 'list_library') {
      return {
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
            outlet: null,
            acquisitionLabel: 'Granted by Manifold',
            acquisitionType: 'GRANT',
          },
        ],
      } as never;
    }
    throw new Error(`Unexpected command: ${command}`);
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
    await screen.findByRole('button', { name: 'Repair installation' }),
  ).toBeInTheDocument();
  expect(screen.getByText('Repair needed')).toBeInTheDocument();
  expect(
    screen.queryByRole('button', { name: 'Play' }),
  ).not.toBeInTheDocument();
});
