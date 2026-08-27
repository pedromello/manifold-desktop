import { invoke } from '@tauri-apps/api/core';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { LibraryPage } from './library';
import { InstallationProvider } from './installation';
import i18n from './i18n';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
  void i18n.changeLanguage('en-US');
});

const ownedGame = {
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
};

const installedGame = {
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
  status: 'INSTALLED',
};

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

it('confirms uninstall, exposes pending state, and returns the game to Install', async () => {
  let completeUninstall: (() => void) | undefined;
  const uninstallRequest = new Promise<void>((resolve) => {
    completeUninstall = resolve;
  });
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    if (command === 'uninstall_game') {
      await uninstallRequest;
      return { gameSlug: 'capyvarias' } as never;
    }
    if (command === 'resolve_latest_release') throw new Error('unavailable');
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

  fireEvent.click(await screen.findByRole('button', { name: 'Uninstall' }));
  expect(
    screen.getByRole('dialog', { name: 'Uninstall Capyvarias?' }),
  ).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
  expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  expect(invoke).not.toHaveBeenCalledWith('uninstall_game', expect.anything());

  fireEvent.click(screen.getByRole('button', { name: 'Uninstall' }));
  fireEvent.click(screen.getByRole('button', { name: 'Uninstall game' }));
  expect(
    screen
      .getAllByRole('button', { name: 'Uninstalling…' })
      .every((button) => button.hasAttribute('disabled')),
  ).toBe(true);
  completeUninstall?.();

  expect(
    await screen.findByRole('button', { name: 'Install' }),
  ).toBeInTheDocument();
  expect(screen.getByText('Capyvarias was uninstalled.')).toBeInTheDocument();
  expect(invoke).toHaveBeenCalledWith('uninstall_game', {
    gameSlug: 'capyvarias',
  });
});

it('keeps the confirmation open with an actionable error when the game is running', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    if (command === 'uninstall_game') {
      throw {
        code: 'GAME_RUNNING',
        message: 'the game is currently running',
        retryable: true,
      };
    }
    if (command === 'resolve_latest_release') throw new Error('unavailable');
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

  fireEvent.click(await screen.findByRole('button', { name: 'Uninstall' }));
  fireEvent.click(screen.getByRole('button', { name: 'Uninstall game' }));

  expect(
    await screen.findByText(
      'Close the game before uninstalling it, then try again.',
    ),
  ).toBeInTheDocument();
  expect(screen.getByRole('dialog')).toBeInTheDocument();
});

it('localizes the uninstall confirmation in Brazilian Portuguese', async () => {
  await i18n.changeLanguage('pt-BR');
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    if (command === 'resolve_latest_release') throw new Error('unavailable');
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

  fireEvent.click(await screen.findByRole('button', { name: 'Desinstalar' }));
  expect(
    screen.getByRole('dialog', { name: 'Desinstalar Capyvarias?' }),
  ).toBeInTheDocument();
  expect(
    screen.getByText(/Sua compra, seu acesso ao jogo/),
  ).toBeInTheDocument();
});
