import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { PublisherAdapter, PublisherGame, PublisherStudio } from './publishing';
import { ReleaseWizardRoute } from './release-wizard';

const studio: PublisherStudio = {
  id: 'studio-1',
  slug: 'studio',
  name: 'Studio',
  description: null,
  logoUrl: null,
  isPublisher: true,
  ownerId: 'user-1',
};

const game: PublisherGame = {
  id: 'game-1',
  slug: 'game',
  title: 'Game',
  description: 'Game description',
  status: 'ACTIVE',
  bannerUrl: null,
  iconUrl: null,
};

const release = {
  id: 'release-1',
  gameId: game.id,
  version: '1.2.0',
  releaseNumber: 7,
  status: 'DRAFT',
  releaseNotes: 'Fresh build',
  publishedAt: null,
  createdAt: '2026-08-26T12:00:00.000Z',
  updatedAt: '2026-08-26T12:00:00.000Z',
};

function seedManifestDraft() {
  window.localStorage.setItem(
    'manifold.publisher.releases.v1',
    JSON.stringify([
      {
        studioSlug: studio.slug,
        gameSlug: game.slug,
        gameTitle: game.title,
        release,
        archivePath: String.raw`C:\Builds\game.zip`,
        inspection: {
          archivePath: String.raw`C:\Builds\game.zip`,
          fileName: 'game.zip',
          compressedSizeBytes: '1024',
          installedSizeBytes: '4096',
          sha256: 'a'.repeat(64),
          executables: ['bin/game.exe', 'bin/crash-handler.exe'],
          suggestedEntrypoint: 'bin/game.exe',
          suggestedWorkingDirectory: 'bin',
        },
        manifest: {
          schemaVersion: '1',
          entrypoint: 'bin/game.exe',
          launchArguments: [],
          workingDirectory: 'bin',
          executables: ['bin/game.exe', 'bin/crash-handler.exe'],
          environment: {},
        },
        uploadStarted: false,
        phase: 'manifest',
        updatedAt: '2026-08-26T12:00:00.000Z',
      },
    ]),
  );
}

function adapter(): PublisherAdapter {
  return {
    listStudios: vi.fn().mockResolvedValue([studio]),
    listGames: vi.fn().mockResolvedValue([game]),
    createDraft: vi.fn(),
    updateDraft: vi.fn(),
    selectArchive: vi.fn(),
    inspectArchive: vi.fn(),
    publish: vi.fn(),
    cancel: vi.fn().mockResolvedValue(undefined),
  };
}

function renderStoredDraft(publishing: PublisherAdapter) {
  return render(
    <MemoryRouter
      initialEntries={['/studio/studio/games/game/releases/release-1']}
    >
      <Routes>
        <Route
          path="/studio/:studioSlug/games/:gameSlug/releases/:releaseId"
          element={
            <ReleaseWizardRoute
              adapter={publishing}
              studios={[studio]}
              onUnauthorized={vi.fn()}
            />
          }
        />
      </Routes>
    </MemoryRouter>,
  );
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

it('locks the reviewed artifact and retries an interrupted upload identically', async () => {
  seedManifestDraft();
  const publishing = adapter();
  const publish = vi
    .fn()
    .mockRejectedValueOnce({
      code: 'UPLOAD_FAILED',
      message: 'storage connection closed',
      retryable: true,
    })
    .mockResolvedValueOnce({
      artifact: { id: 'artifact-1', status: 'READY' },
      release: {
        ...release,
        status: 'PUBLISHED',
        publishedAt: '2026-08-26T12:05:00.000Z',
        updatedAt: '2026-08-26T12:05:00.000Z',
      },
      published: true,
    });
  publishing.publish = publish;

  renderStoredDraft(publishing);
  fireEvent.click(await screen.findByRole('button', { name: 'Send version' }));

  const retry = await screen.findByRole('button', { name: 'Try again' });
  expect(screen.getByRole('combobox')).toBeDisabled();
  expect(screen.getByRole('button', { name: 'Back' })).toBeDisabled();
  fireEvent.click(retry);

  expect(
    await screen.findByRole('heading', { name: 'Ready for players.' }),
  ).toBeInTheDocument();
  expect(publish).toHaveBeenCalledTimes(2);
  expect(publish.mock.calls[1]?.slice(0, 3)).toEqual(
    publish.mock.calls[0]?.slice(0, 3),
  );
});

it('cancels an active upload and returns to the retryable review state', async () => {
  seedManifestDraft();
  const publishing = adapter();
  publishing.publish = vi.fn(
    async (releaseId, _archivePath, _manifest, onProgress) => {
      onProgress({
        releaseId,
        phase: 'uploading',
        uploadedBytes: 512,
        totalBytes: 1024,
        attempt: 1,
      });
      return new Promise<never>(() => {});
    },
  );

  renderStoredDraft(publishing);
  fireEvent.click(await screen.findByRole('button', { name: 'Send version' }));
  fireEvent.click(
    await screen.findByRole('button', { name: 'Cancel sending' }),
  );

  expect(publishing.cancel).toHaveBeenCalledWith('release-1');
  expect(
    await screen.findByRole('button', { name: 'Try again' }),
  ).toBeInTheDocument();
});
