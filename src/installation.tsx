import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import {
  createContext,
  PropsWithChildren,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from 'react';
import { DownloadMetricState, updateDownloadMetrics } from './download-metrics';
import {
  DistributionAdapter,
  DistributionPlan,
  productionDistributionAdapter,
} from './distribution';
import {
  DistributionErrorCode,
  normalizeDistributionFailure,
} from './distribution-errors';
import type { ReleaseSummary } from './contracts/desktop-v1';
import { installWithAuthorizationRefresh } from './installation-retry';

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
  errorCode?: DistributionErrorCode;
  retryable?: boolean;
  bytesPerSecond?: number;
  estimatedSecondsRemaining?: number;
};

type GameProcessState = {
  gameSlug: string;
  running: boolean;
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

export type UninstallErrorCode =
  | 'GAME_RUNNING'
  | 'INSTALLATION_ACTIVE'
  | 'NOT_INSTALLED'
  | 'UNSAFE_INSTALLATION_PATH'
  | 'LOCAL_STATE_ERROR'
  | 'FILESYSTEM_ERROR'
  | 'UNINSTALL_FAILED';

export type UninstallFailure = {
  code: UninstallErrorCode;
  message: string;
  retryable: boolean;
};

export function normalizeUninstallFailure(reason: unknown): UninstallFailure {
  if (reason && typeof reason === 'object') {
    const value = reason as Record<string, unknown>;
    if (typeof value.code === 'string') {
      return {
        code: value.code as UninstallErrorCode,
        message:
          typeof value.message === 'string'
            ? value.message
            : 'uninstall failed',
        retryable: value.retryable !== false,
      };
    }
  }
  return {
    code: 'UNINSTALL_FAILED',
    message: typeof reason === 'string' ? reason : 'uninstall failed',
    retryable: true,
  };
}

type InstallationContextValue = {
  progress: Record<string, InstallationProgress>;
  installed: Record<string, InstalledGame>;
  playing: Record<string, boolean>;
  availableUpdates: Record<string, string>;
  install: (gameSlug: string, title: string) => Promise<void>;
  cancel: (gameSlug: string) => Promise<void>;
  launch: (gameSlug: string) => Promise<void>;
  uninstall: (gameSlug: string) => Promise<void>;
  refresh: () => Promise<void>;
  checkForUpdates: (gameSlugs: string[]) => Promise<void>;
};

const InstallationContext = createContext<InstallationContextValue | null>(
  null,
);

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
  const [playing, setPlaying] = useState<Record<string, boolean>>({});
  const [availableUpdates, setAvailableUpdates] = useState<
    Record<string, string>
  >({});
  const downloadMetrics = useRef<Record<string, DownloadMetricState>>({});

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

  const refreshPlaying = useCallback(async () => {
    try {
      const gameSlugs = await invoke<string[]>('list_running_games');
      setPlaying(
        Object.fromEntries(gameSlugs.map((gameSlug) => [gameSlug, true])),
      );
    } catch {
      setPlaying({});
    }
  }, []);

  useEffect(() => {
    void refresh();
    void refreshPlaying();
    let active = true;
    const disposers: Array<() => void> = [];
    void listen<InstallationProgress>('installation-progress', (event) => {
      if (!active) return;
      const update = event.payload;
      if (update.phase === 'downloading') {
        const metrics = updateDownloadMetrics(
          downloadMetrics.current[update.gameSlug],
          update.downloadedBytes,
          update.totalBytes,
          performance.now(),
        );
        downloadMetrics.current[update.gameSlug] = metrics.state;
        setProgress((current) => ({
          ...current,
          [update.gameSlug]: {
            ...update,
            bytesPerSecond: metrics.bytesPerSecond,
            estimatedSecondsRemaining: metrics.estimatedSecondsRemaining,
          },
        }));
      } else {
        delete downloadMetrics.current[update.gameSlug];
        setProgress((current) => ({ ...current, [update.gameSlug]: update }));
      }
      if (update.phase === 'installed') void refresh();
    })
      .then((unlisten) => {
        if (active) disposers.push(unlisten);
        else unlisten();
      })
      .catch(() => undefined);
    void listen<GameProcessState>('game-process-state', (event) => {
      if (!active) return;
      const update = event.payload;
      setPlaying((current) => {
        const next = { ...current };
        if (update.running) next[update.gameSlug] = true;
        else delete next[update.gameSlug];
        return next;
      });
    })
      .then((unlisten) => {
        if (active) disposers.push(unlisten);
        else unlisten();
      })
      .catch(() => undefined);
    return () => {
      active = false;
      disposers.forEach((dispose) => dispose());
    };
  }, [refresh, refreshPlaying]);

  const install = useCallback(
    async (gameSlug: string, title: string) => {
      delete downloadMetrics.current[gameSlug];
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
      try {
        await installWithAuthorizationRefresh(
          adapter,
          gameSlug,
          title,
          (resolvedTitle, plan: DistributionPlan) =>
            invoke<InstalledGame>('install_game', {
              title: resolvedTitle,
              plan,
            }),
        );
        await refresh();
      } catch (reason) {
        const failure = normalizeDistributionFailure(reason);
        const cancelled = failure.message.toLowerCase().includes('cancelled');
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
            error: cancelled ? null : failure.message,
            errorCode: cancelled ? undefined : failure.code,
            retryable: cancelled ? undefined : failure.retryable,
          },
        }));
      }
    },
    [adapter, refresh],
  );

  const cancel = useCallback(async (gameSlug: string) => {
    await invoke('cancel_installation', { gameSlug });
  }, []);

  const launch = useCallback(
    async (gameSlug: string) => {
      await invoke('launch_game', { gameSlug });
      await refreshPlaying();
    },
    [refreshPlaying],
  );

  const uninstall = useCallback(async (gameSlug: string) => {
    await invoke('uninstall_game', { gameSlug });
    delete downloadMetrics.current[gameSlug];
    setInstalled((current) => {
      const next = { ...current };
      delete next[gameSlug];
      return next;
    });
    setAvailableUpdates((current) => {
      const next = { ...current };
      delete next[gameSlug];
      return next;
    });
    setProgress((current) => {
      const next = { ...current };
      delete next[gameSlug];
      return next;
    });
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
      playing,
      availableUpdates,
      install,
      cancel,
      launch,
      uninstall,
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
      playing,
      progress,
      refresh,
      uninstall,
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
