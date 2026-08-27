import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';

export type PublisherStudio = {
  id: string;
  slug: string;
  name: string;
  description: string | null;
  logoUrl: string | null;
  isPublisher: boolean;
  ownerId: string;
};

export type PublisherGame = {
  id: string;
  slug: string;
  title: string;
  description: string;
  status: string;
  bannerUrl: string | null;
  iconUrl: string | null;
};

export type PublisherRelease = {
  id: string;
  gameId: string;
  version: string;
  releaseNumber: number;
  status: string;
  releaseNotes: string | null;
  publishedAt: string | null;
  createdAt: string;
  updatedAt: string;
};

export type ArchiveInspection = {
  archivePath: string;
  fileName: string;
  compressedSizeBytes: string;
  installedSizeBytes: string;
  sha256: string;
  executables: string[];
  suggestedEntrypoint: string | null;
  suggestedWorkingDirectory: string | null;
};

export type PublishManifest = {
  schemaVersion: '1';
  entrypoint: string;
  launchArguments: string[];
  workingDirectory: string | null;
  executables: string[];
  environment: Record<string, string>;
};

export type PublisherProgress = {
  releaseId: string;
  phase:
    | 'analyzing'
    | 'uploading'
    | 'verifying'
    | 'published'
    | 'failed'
    | 'cancelled';
  uploadedBytes: number;
  totalBytes: number;
  attempt: number;
};

export type PublishConfirmation = {
  artifact: { id: string; status: string };
  release: PublisherRelease;
  published: boolean;
};

export type PublisherError = {
  code: string;
  message: string;
  retryable: boolean;
};

export function isPublisherError(value: unknown): value is PublisherError {
  return (
    typeof value === 'object' &&
    value !== null &&
    'code' in value &&
    'message' in value &&
    typeof value.code === 'string' &&
    typeof value.message === 'string'
  );
}

export interface PublisherAdapter {
  listStudios(): Promise<PublisherStudio[]>;
  listGames(studioSlug: string): Promise<PublisherGame[]>;
  createDraft(
    gameSlug: string,
    version: string,
    releaseNotes: string | null,
  ): Promise<PublisherRelease>;
  updateDraft(
    gameSlug: string,
    releaseId: string,
    version: string,
    releaseNotes: string | null,
  ): Promise<PublisherRelease>;
  selectArchive(): Promise<string | null>;
  inspectArchive(archivePath: string): Promise<ArchiveInspection>;
  publish(
    releaseId: string,
    archivePath: string,
    manifest: PublishManifest,
    onProgress: (progress: PublisherProgress) => void,
  ): Promise<PublishConfirmation>;
  cancel(releaseId: string): Promise<void>;
}

export const nativePublisherAdapter: PublisherAdapter = {
  async listStudios() {
    return invoke<PublisherStudio[]>('list_publisher_studios');
  },
  async listGames(studioSlug) {
    return invoke<PublisherGame[]>('list_studio_games', { studioSlug });
  },
  createDraft(gameSlug, version, releaseNotes) {
    return invoke<PublisherRelease>('create_release_draft', {
      gameSlug,
      version,
      releaseNotes,
    });
  },
  updateDraft(gameSlug, releaseId, version, releaseNotes) {
    return invoke<PublisherRelease>('update_release_draft', {
      gameSlug,
      releaseId,
      version,
      releaseNotes,
    });
  },
  async selectArchive() {
    const selected = await open({
      directory: false,
      multiple: false,
      filters: [{ name: 'ZIP', extensions: ['zip'] }],
    });
    return typeof selected === 'string' ? selected : null;
  },
  inspectArchive(archivePath) {
    return invoke<ArchiveInspection>('inspect_publish_archive', {
      archivePath,
    });
  },
  async publish(releaseId, archivePath, manifest, onProgress) {
    const unlisten = await listen<PublisherProgress>(
      'publisher-progress',
      ({ payload }) => {
        if (payload.releaseId === releaseId) onProgress(payload);
      },
    );
    try {
      return await invoke<PublishConfirmation>('publish_release', {
        releaseId,
        archivePath,
        manifest,
      });
    } finally {
      unlisten();
    }
  },
  cancel(releaseId) {
    return invoke('cancel_publish_upload', { releaseId });
  },
};

const fixtureStudio: PublisherStudio = {
  id: 'fixture-studio',
  slug: 'manifold-labs',
  name: 'Manifold Labs',
  description: '',
  logoUrl: null,
  isPublisher: true,
  ownerId: 'fixture-owner',
};

const fixtureGame: PublisherGame = {
  id: 'fixture-game',
  slug: 'peggys-post',
  title: "Peggy's Post",
  description: '',
  status: 'ACTIVE',
  bannerUrl: null,
  iconUrl: null,
};

let fixtureReleaseNumber = 1;

function fixtureText(portuguese: string, english: string) {
  return document.documentElement.lang === 'pt-BR' ? portuguese : english;
}

export const fixturePublisherAdapter: PublisherAdapter = {
  async listStudios() {
    return [
      {
        ...fixtureStudio,
        description: fixtureText(
          'Dados locais para revisão visual',
          'Local data for visual review',
        ),
      },
    ];
  },
  async listGames() {
    return [
      {
        ...fixtureGame,
        description: fixtureText(
          'Uma versão para Windows pronta para publicação.',
          'A Windows version ready to publish.',
        ),
      },
    ];
  },
  async createDraft(_gameSlug, version, releaseNotes) {
    const now = new Date().toISOString();
    return {
      id: `fixture-release-${fixtureReleaseNumber}`,
      gameId: fixtureGame.id,
      version,
      releaseNumber: fixtureReleaseNumber++,
      status: 'DRAFT',
      releaseNotes,
      publishedAt: null,
      createdAt: now,
      updatedAt: now,
    };
  },
  async updateDraft(_gameSlug, releaseId, version, releaseNotes) {
    const now = new Date().toISOString();
    return {
      id: releaseId,
      gameId: fixtureGame.id,
      version,
      releaseNumber: 1,
      status: 'DRAFT',
      releaseNotes,
      publishedAt: null,
      createdAt: now,
      updatedAt: now,
    };
  },
  async selectArchive() {
    return "C:\\Fixture\\Peggy's Post Windows.zip";
  },
  async inspectArchive(archivePath) {
    return {
      archivePath,
      fileName: "Peggy's Post Windows.zip",
      compressedSizeBytes: '71059858',
      installedSizeBytes: '305505736',
      sha256:
        'ab65ef4cb6d6b24f4173a1258800f73df26d4310774fc46a6c99c32bb011bf7a',
      executables: [
        "Peggy's Post/Peggy's Post.exe",
        "Peggy's Post/UnityCrashHandler64.exe",
      ],
      suggestedEntrypoint: "Peggy's Post/Peggy's Post.exe",
      suggestedWorkingDirectory: "Peggy's Post",
    };
  },
  async publish(releaseId, _archivePath, _manifest, onProgress) {
    const totalBytes = 71059858;
    for (const uploadedBytes of [8_000_000, 32_000_000, totalBytes]) {
      onProgress({
        releaseId,
        phase: 'uploading',
        uploadedBytes,
        totalBytes,
        attempt: 1,
      });
      await new Promise((resolve) => window.setTimeout(resolve, 180));
    }
    onProgress({
      releaseId,
      phase: 'verifying',
      uploadedBytes: totalBytes,
      totalBytes,
      attempt: 1,
    });
    await new Promise((resolve) => window.setTimeout(resolve, 350));
    const now = new Date().toISOString();
    return {
      artifact: { id: 'fixture-artifact', status: 'READY' },
      release: {
        id: releaseId,
        gameId: fixtureGame.id,
        version: '1.5.9',
        releaseNumber: 1,
        status: 'PUBLISHED',
        releaseNotes: null,
        publishedAt: now,
        createdAt: now,
        updatedAt: now,
      },
      published: true,
    };
  },
  async cancel() {},
};

export const publisherFixturesEnabled =
  import.meta.env.DEV && import.meta.env.VITE_PUBLISHER_FIXTURES === 'true';
export const publisherAdapter = publisherFixturesEnabled
  ? fixturePublisherAdapter
  : nativePublisherAdapter;

export type StoredPublisherRelease = {
  studioSlug: string;
  gameSlug: string;
  gameTitle: string;
  release: PublisherRelease;
  archivePath: string | null;
  inspection: ArchiveInspection | null;
  manifest: PublishManifest | null;
  uploadStarted?: boolean;
  phase:
    | 'details'
    | 'file'
    | 'preflight'
    | 'manifest'
    | 'uploading'
    | 'verifying'
    | 'success';
  updatedAt: string;
};

const RECOVERY_KEY = 'manifold.publisher.releases.v1';

export function loadStoredPublisherReleases(): StoredPublisherRelease[] {
  try {
    const raw = window.localStorage.getItem(RECOVERY_KEY);
    if (!raw) return [];
    const values = JSON.parse(raw) as StoredPublisherRelease[];
    if (!Array.isArray(values)) return [];
    return values.map((value) =>
      value.phase === 'preflight' ||
      value.phase === 'uploading' ||
      value.phase === 'verifying'
        ? { ...value, phase: 'manifest' as const }
        : value,
    );
  } catch {
    return [];
  }
}

export function storePublisherRelease(value: StoredPublisherRelease) {
  const values = loadStoredPublisherReleases().filter(
    (candidate) => candidate.release.id !== value.release.id,
  );
  values.unshift({ ...value, updatedAt: new Date().toISOString() });
  window.localStorage.setItem(RECOVERY_KEY, JSON.stringify(values));
}

export function findStoredPublisherRelease(releaseId: string) {
  return loadStoredPublisherReleases().find(
    (value) => value.release.id === releaseId,
  );
}

export function parentDirectory(path: string) {
  const normalized = path.replaceAll('\\\\', '/').replaceAll('\\', '/');
  const index = normalized.lastIndexOf('/');
  return index > 0 ? normalized.slice(0, index) : null;
}
