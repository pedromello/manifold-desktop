import { invoke } from '@tauri-apps/api/core';
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { LibraryPage } from './library';
import { InstallationProvider } from './installation';
import i18n from './i18n';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const eventListeners = vi.hoisted(
  () => new Map<string, (event: { payload: unknown }) => void>(),
);
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn(
    async (event: string, listener: (event: { payload: unknown }) => void) => {
      eventListeners.set(event, listener);
      return () => eventListeners.delete(event);
    },
  ),
}));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
  eventListeners.clear();
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
  status: 'ACTIVE',
  purchaseMode: 'PLATFORM',
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
        acquisitionType: 'OUTLET',
        status: 'ACTIVE',
        purchaseMode: 'PLATFORM',
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
            status: 'ACTIVE',
            purchaseMode: 'PLATFORM',
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

it('shows Playing until the launched game process exits', async () => {
  let running = false;
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_running_games') {
      return (running ? ['capyvarias'] : []) as never;
    }
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    if (command === 'launch_game') {
      running = true;
      return undefined as never;
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

  fireEvent.click(await screen.findByRole('button', { name: 'Play' }));
  const playingButton = await screen.findByRole('button', { name: 'Playing' });
  expect(playingButton).toBeDisabled();
  expect(invoke).toHaveBeenCalledWith('launch_game', {
    gameSlug: 'capyvarias',
  });
  expect(
    vi
      .mocked(invoke)
      .mock.calls.filter(([command]) => command === 'launch_game'),
  ).toHaveLength(1);

  running = false;
  act(() => {
    eventListeners.get('game-process-state')?.({
      payload: { gameSlug: 'capyvarias', running: false },
    });
  });

  await waitFor(() =>
    expect(screen.getByRole('button', { name: 'Play' })).toBeEnabled(),
  );
});

it('restores Playing from the native query after a WebView reload', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_running_games') return ['capyvarias'] as never;
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

  expect(await screen.findByRole('button', { name: 'Playing' })).toBeDisabled();
  expect(invoke).not.toHaveBeenCalledWith('launch_game', expect.anything());
});

it('does not leave Playing active when process creation fails', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') return [installedGame] as never;
    if (command === 'list_running_games') return [] as never;
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    if (command === 'launch_game') throw new Error('spawn failed');
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

  fireEvent.click(await screen.findByRole('button', { name: 'Play' }));

  expect(
    await screen.findByText('The game could not be launched.'),
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Play' })).toBeEnabled();
  expect(
    screen.queryByRole('button', { name: 'Playing' }),
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

it('does not offer local installation for a display-only Steam game', async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations' || command === 'list_running_games') {
      return [] as never;
    }
    if (command === 'list_library') {
      return {
        total: 1,
        games: [
          {
            ...ownedGame,
            id: 'steam-game',
            libraryId: 'steam-library',
            slug: 'steam-game',
            title: 'Steam Game',
            status: 'ONLY_DISPLAY',
            purchaseMode: 'STEAM_ONLY',
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
    await screen.findByRole('button', { name: 'No Manifold download' }),
  ).toBeDisabled();
  expect(
    screen.queryByRole('button', { name: 'Install' }),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole('button', { name: 'Uninstall' }),
  ).not.toBeInTheDocument();
});
