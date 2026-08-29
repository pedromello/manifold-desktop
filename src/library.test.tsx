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
import type { UpdatePlan } from './contracts/desktop-v1';
import type { DistributionAdapter, UpdateExecutionPlan } from './distribution';
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
  vi.restoreAllMocks();
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

const sourceReleaseId = '11111111-1111-4111-8111-111111111111';
const targetReleaseId = '22222222-2222-4222-8222-222222222222';
const targetArtifactId = '33333333-3333-4333-8333-333333333333';
const patchId = '44444444-4444-4444-8444-444444444444';
const artifactSha = 'a'.repeat(64);
const patchSha = 'b'.repeat(64);
const signatureSha = 'c'.repeat(64);

const patchUpdatePlan: UpdatePlan = {
  strategy: 'PATCH',
  source: { id: sourceReleaseId, version: '1.0.0', release_number: 1 },
  target: {
    id: targetReleaseId,
    version: '2.0.0',
    release_number: 2,
    published_at: '2026-08-28T00:00:00.000Z',
    artifact_id: targetArtifactId,
    target: { platform: 'WINDOWS', architecture: 'X86_64' },
    compressed_size_bytes: '100000000',
    installed_size_bytes: '200000000',
    sha256: artifactSha,
    manifest_schema_version: '1',
  },
  patch: {
    id: patchId,
    source_release_id: sourceReleaseId,
    target_release_id: targetReleaseId,
    target: { platform: 'WINDOWS', architecture: 'X86_64' },
    algorithm: 'WHARF',
    format_version: '1',
    status: 'READY',
    patch: { size_bytes: '40000000', sha256: patchSha },
    signature: { size_bytes: '1024', sha256: signatureSha },
    expected_installation_sha256: signatureSha,
    generation_duration_ms: '1000',
    created_at: '2026-08-28T00:00:00.000Z',
    updated_at: '2026-08-28T00:00:00.000Z',
  },
  fallback_artifact_id: targetArtifactId,
};

function hydratedPatch(urlSuffix: string, etag: string): UpdateExecutionPlan {
  return {
    update: patchUpdatePlan,
    manifest: {
      schema_version: '1',
      release_id: targetReleaseId,
      artifact_id: targetArtifactId,
      entrypoint: 'Capyvarias.exe',
      launch_arguments: [],
      executables: ['Capyvarias.exe'],
      environment: {},
    },
    patch_downloads: {
      patch: {
        patch_id: patchId,
        file: 'PATCH',
        url: `https://downloads.test/${urlSuffix}.pwr`,
        expires_at: '2026-08-28T01:00:00.000Z',
        total_size_bytes: '40000000',
        sha256: patchSha,
        etag,
      },
      signature: {
        patch_id: patchId,
        file: 'SIGNATURE',
        url: `https://downloads.test/${urlSuffix}.pwr.sig`,
        expires_at: '2026-08-28T01:00:00.000Z',
        total_size_bytes: '1024',
        sha256: signatureSha,
        etag: `${etag}-signature`,
      },
    },
    fallback_download: {
      artifact_id: targetArtifactId,
      url: 'https://downloads.test/full.zip',
      expires_at: '2026-08-28T01:00:00.000Z',
      total_size_bytes: '100000000',
      sha256: artifactSha,
      etag: 'full-v1',
    },
  };
}

function incrementalAdapter(
  prepareUpdate: DistributionAdapter['prepareUpdate'] = vi
    .fn<DistributionAdapter['prepareUpdate']>()
    .mockResolvedValue(hydratedPatch('first', 'patch-v1')),
): DistributionAdapter {
  return {
    latest: vi.fn(async () => patchUpdatePlan.target),
    resolve: vi.fn(async () => {
      throw new Error('not used');
    }),
    resolveUpdate: vi.fn(async () => patchUpdatePlan),
    prepareUpdate,
  };
}

function mockIncrementalLibraryNative() {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === 'list_installations') {
      return [
        {
          ...installedGame,
          releaseId: sourceReleaseId,
          releaseNumber: 1,
          artifactId: '55555555-5555-4555-8555-555555555555',
        },
      ] as never;
    }
    if (command === 'list_running_games') return [] as never;
    if (command === 'list_library') {
      return { total: 1, games: [ownedGame] } as never;
    }
    throw new Error(`Unexpected command: ${command}`);
  });
}

it('shows patch size, savings, every update phase, and checks on refresh and interval', async () => {
  mockIncrementalLibraryNative();
  const adapter = incrementalAdapter();
  let updateInterval: TimerHandler | undefined;
  vi.spyOn(window, 'setInterval').mockImplementation((handler, delay) => {
    if (delay === 15 * 60 * 1000) updateInterval = handler;
    return 71;
  });

  render(
    <MemoryRouter>
      <InstallationProvider adapter={adapter}>
        <LibraryPage
          user={{ id: 'user-1', username: 'pedro', email: 'pedro@example.com' }}
          onAuthenticated={vi.fn()}
          onSessionExpired={vi.fn()}
        />
      </InstallationProvider>
    </MemoryRouter>,
  );

  expect(
    await screen.findByText('Version 2.0.0 available'),
  ).toBeInTheDocument();
  expect(screen.getByText(/38.1 MB patch · 60% savings/)).toBeInTheDocument();
  const initialChecks = vi.mocked(adapter.resolveUpdate).mock.calls.length;
  expect(initialChecks).toBeGreaterThan(0);

  fireEvent.click(screen.getByRole('button', { name: 'Refresh' }));
  await waitFor(() =>
    expect(vi.mocked(adapter.resolveUpdate).mock.calls.length).toBeGreaterThan(
      initialChecks,
    ),
  );
  const afterRefresh = vi.mocked(adapter.resolveUpdate).mock.calls.length;
  expect(updateInterval).toBeTypeOf('function');
  act(() => {
    if (typeof updateInterval === 'function') updateInterval();
  });
  await waitFor(() =>
    expect(vi.mocked(adapter.resolveUpdate).mock.calls.length).toBeGreaterThan(
      afterRefresh,
    ),
  );

  const phases = [
    ['preparing_update', 'Preparing update'],
    ['downloading_update', 'Downloading update'],
    ['applying_update', 'Applying update'],
    ['verifying_update', 'Verifying update'],
    ['full_fallback', 'Safely downloading the full package'],
  ] as const;
  for (const [phase, label] of phases) {
    act(() => {
      eventListeners.get('installation-progress')?.({
        payload: {
          gameSlug: 'capyvarias',
          title: 'Capyvarias',
          phase,
          downloadedBytes: 1,
          totalBytes: 2,
          version: '2.0.0',
          error: null,
        },
      });
    });
    expect(screen.getByRole('button', { name: label })).toBeDisabled();
  }
});

it('renews patch and signature authorizations once before the native full fallback', async () => {
  mockIncrementalLibraryNative();
  const prepareUpdate = vi
    .fn<DistributionAdapter['prepareUpdate']>()
    .mockResolvedValueOnce(hydratedPatch('first', 'patch-v1'))
    .mockResolvedValueOnce(hydratedPatch('second', 'patch-v2'));
  const adapter = incrementalAdapter(prepareUpdate);
  let updateAttempts = 0;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'list_installations') {
      return [
        {
          ...installedGame,
          releaseId: sourceReleaseId,
          releaseNumber: 1,
          artifactId: '55555555-5555-4555-8555-555555555555',
        },
      ] as never;
    }
    if (command === 'list_running_games') return [] as never;
    if (command === 'list_library')
      return { total: 1, games: [ownedGame] } as never;
    if (command === 'update_game') {
      updateAttempts += 1;
      if (updateAttempts === 1) {
        throw {
          code: 'DOWNLOAD_AUTHORIZATION_EXPIRED',
          message: 'download authorization expired',
          retryable: true,
        };
      }
      return {
        ...installedGame,
        version: '2.0.0',
        releaseId: targetReleaseId,
      } as never;
    }
    throw new Error(`Unexpected command: ${command} ${JSON.stringify(args)}`);
  });

  render(
    <MemoryRouter>
      <InstallationProvider adapter={adapter}>
        <LibraryPage
          user={{ id: 'user-1', username: 'pedro', email: 'pedro@example.com' }}
          onAuthenticated={vi.fn()}
          onSessionExpired={vi.fn()}
        />
      </InstallationProvider>
    </MemoryRouter>,
  );

  fireEvent.click(await screen.findByRole('button', { name: 'Update' }));
  await waitFor(() => expect(updateAttempts).toBe(2));
  expect(prepareUpdate).toHaveBeenCalledTimes(2);
  const updateCalls = vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === 'update_game');
  expect(updateCalls[0]?.[1]).toMatchObject({
    authorizationRefreshAttempted: false,
    plan: {
      patch_downloads: { patch: { url: 'https://downloads.test/first.pwr' } },
    },
  });
  expect(updateCalls[1]?.[1]).toMatchObject({
    authorizationRefreshAttempted: true,
    plan: {
      patch_downloads: { patch: { url: 'https://downloads.test/second.pwr' } },
    },
  });
  expect(adapter.resolveUpdate).toHaveBeenCalledWith(
    'capyvarias',
    sourceReleaseId,
  );
});
