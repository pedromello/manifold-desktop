import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react';
import {
  DistributionAdapter,
  DistributionPlan,
  productionDistributionAdapter,
} from './distribution';
import type { ReleaseSummary } from './contracts/desktop-v1';

export type InstallationPhase =
  | 'queued'
  | 'resolving'
  | 'downloading'
  | 'verifying'
  | 'extracting'
  | 'installing'
  | 'installed'
  | 'failed'
  | 'cancelled';

export type InstallationProgress = {
  gameSlug: string;
  title: string;
  phase: InstallationPhase;
  downloadedBytes: number;
  totalBytes: number;
  version: string | null;
  error: string | null;
};

export type InstalledGame = {
  gameSlug: string;
  title: string;
  version: string;
  releaseId: string;
  releaseNumber: number;
  artifactId: string;
  installedSizeBytes: string;
  installDirectory: string;
  entrypoint: string;
  installedAt: string;
  status: 'INSTALLED' | 'REPAIR_NEEDED';
};

export type InstallationPreferences = {
  installDirectory: string | null;
  defaultInstallDirectory: string;
};

type InstallationContextValue = {
  progress: Record<string, InstallationProgress>;
  installed: Record<string, InstalledGame>;
  availableUpdates: Record<string, string>;
  install: (gameSlug: string, title: string) => Promise<void>;
  cancel: (gameSlug: string) => Promise<void>;
  launch: (gameSlug: string) => Promise<void>;
  refresh: () => Promise<void>;
  checkForUpdates: (gameSlugs: string[]) => Promise<void>;
};

const InstallationContext = createContext<InstallationContextValue | null>(
  null,
);

function message(reason: unknown) {
  return typeof reason === 'string'
    ? reason
    : reason instanceof Error
      ? reason.message
      : 'Installation failed';
}

export function isUpdateAvailable(
  current: InstalledGame,
  release: ReleaseSummary,
) {
  return (
    current.status === 'INSTALLED' &&
    release.id !== current.releaseId &&
    release.release_number > current.releaseNumber
  );
}

export function InstallationProvider({
  children,
  adapter = productionDistributionAdapter,
}: PropsWithChildren<{ adapter?: DistributionAdapter }>) {
  const [progress, setProgress] = useState<
    Record<string, InstallationProgress>
  >({});
  const [installed, setInstalled] = useState<Record<string, InstalledGame>>({});
  const [availableUpdates, setAvailableUpdates] = useState<
    Record<string, string>
  >({});

  const refresh = useCallback(async () => {
    try {
      const games = await invoke<InstalledGame[]>('list_installations');
      setInstalled(
        Object.fromEntries(games.map((game) => [game.gameSlug, game])),
      );
    } catch {
      setInstalled({});
    }
  }, []);

  useEffect(() => {
    void refresh();
    let active = true;
    let dispose: (() => void) | undefined;
    void listen<InstallationProgress>('installation-progress', (event) => {
      if (!active) return;
      const update = event.payload;
      setProgress((current) => ({ ...current, [update.gameSlug]: update }));
      if (update.phase === 'installed') void refresh();
    })
      .then((unlisten) => {
        if (active) dispose = unlisten;
        else unlisten();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      dispose?.();
    };
  }, [refresh]);

  const install = useCallback(
    async (gameSlug: string, title: string) => {
      setProgress((current) => ({
        ...current,
        [gameSlug]: {
          gameSlug,
          title,
          phase: 'resolving',
          downloadedBytes: 0,
          totalBytes: 0,
          version: null,
          error: null,
        },
      }));
      let plan: DistributionPlan;
      try {
        plan = await adapter.resolve(gameSlug);
        await invoke<InstalledGame>('install_game', { title, plan });
        await refresh();
      } catch (reason) {
        const cancelled = message(reason).toLowerCase().includes('cancelled');
        setProgress((current) => ({
          ...current,
          [gameSlug]: {
            ...(current[gameSlug] ?? {
              gameSlug,
              title,
              downloadedBytes: 0,
              totalBytes: 0,
              version: null,
            }),
            phase: cancelled ? 'cancelled' : 'failed',
            error: cancelled ? null : message(reason),
          },
        }));
      }
    },
    [adapter, refresh],
  );

  const cancel = useCallback(async (gameSlug: string) => {
    await invoke('cancel_installation', { gameSlug });
  }, []);

  const launch = useCallback(async (gameSlug: string) => {
    await invoke('launch_game', { gameSlug });
  }, []);

  const checkForUpdates = useCallback(
    async (gameSlugs: string[]) => {
      const updates: Record<string, string> = {};
      await Promise.allSettled(
        gameSlugs.map(async (gameSlug) => {
          const current = installed[gameSlug];
          if (!current) return;
          const release = await adapter.latest(gameSlug);
          if (isUpdateAvailable(current, release)) {
            updates[gameSlug] = release.version;
          }
        }),
      );
      setAvailableUpdates(updates);
    },
    [adapter, installed],
  );

  const value = useMemo(
    () => ({
      progress,
      installed,
      availableUpdates,
      install,
      cancel,
      launch,
      refresh,
      checkForUpdates,
    }),
    [
      availableUpdates,
      cancel,
      checkForUpdates,
      install,
      installed,
      launch,
      progress,
      refresh,
    ],
  );
  return (
    <InstallationContext.Provider value={value}>
      {children}
    </InstallationContext.Provider>
  );
}

export function useInstallations() {
  const context = useContext(InstallationContext);
  if (!context)
    throw new Error(
      'useInstallations must be used inside InstallationProvider',
    );
  return context;
}
