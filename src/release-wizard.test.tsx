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

function adapter(): PublisherAdapter {
  return {
    listStudios: vi.fn().mockResolvedValue([studio]),
    listGames: vi.fn().mockResolvedValue([game]),
    createDraft: vi.fn().mockResolvedValue({
      id: 'release-1',
      gameId: game.id,
      version: '1.2.0',
      releaseNumber: 7,
      status: 'DRAFT',
      releaseNotes: 'Fresh build',
      publishedAt: null,
      createdAt: '2026-08-26T12:00:00.000Z',
      updatedAt: '2026-08-26T12:00:00.000Z',
    }),
    selectArchive: vi.fn().mockResolvedValue(String.raw`C:\Builds\game.zip`),
    inspectArchive: vi.fn().mockResolvedValue({
      archivePath: String.raw`C:\Builds\game.zip`,
      fileName: 'game.zip',
      compressedSizeBytes: '1024',
      installedSizeBytes: '4096',
      sha256: 'a'.repeat(64),
      executables: ['bin/game.exe', 'bin/crash-handler.exe'],
      suggestedEntrypoint: 'bin/game.exe',
      suggestedWorkingDirectory: 'bin',
    }),
    publish: vi
      .fn()
      .mockImplementation(
        async (
          releaseId: string,
          _archivePath: string,
          _manifest: unknown,
          onProgress: (value: {
            releaseId: string;
            phase: 'uploading' | 'verifying';
            uploadedBytes: number;
            totalBytes: number;
            attempt: number;
          }) => void,
        ) => {
          onProgress({
            releaseId,
            phase: 'uploading',
            uploadedBytes: 512,
            totalBytes: 1024,
            attempt: 1,
          });
          onProgress({
            releaseId,
            phase: 'verifying',
            uploadedBytes: 1024,
            totalBytes: 1024,
            attempt: 1,
          });
          return {
            artifact: { id: 'artifact-1', status: 'READY' },
            release: {
              id: releaseId,
              gameId: game.id,
              version: '1.2.0',
              releaseNumber: 7,
              status: 'PUBLISHED',
              releaseNotes: 'Fresh build',
              publishedAt: '2026-08-26T12:05:00.000Z',
              createdAt: '2026-08-26T12:00:00.000Z',
              updatedAt: '2026-08-26T12:05:00.000Z',
            },
            published: true,
          };
        },
      ),
    cancel: vi.fn().mockResolvedValue(undefined),
  };
}

afterEach(() => {
  cleanup();
  window.localStorage.clear();
});

it('runs the readable draft, file, preflight, manifest and publication stages', async () => {
  const publishing = adapter();
  render(
    <MemoryRouter initialEntries={['/studio/studio/games/game/releases/new']}>
      <Routes>
        <Route
          path="/studio/:studioSlug/games/:gameSlug/releases/new"
          element={
            <ReleaseWizardRoute
              adapter={publishing}
              studios={[studio]}
              onUnauthorized={vi.fn()}
            />
          }
        />
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

  expect(
    await screen.findByRole('heading', { name: 'Release details' }),
  ).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText('Version'), {
    target: { value: '1.2.0' },
  });
  fireEvent.change(screen.getByLabelText('Release notes'), {
    target: { value: 'Fresh build' },
  });
  fireEvent.click(
    screen.getByRole('button', { name: 'Create draft and continue' }),
  );

  fireEvent.click(await screen.findByRole('button', { name: 'Select ZIP' }));
  expect(await screen.findByText('game.zip')).toBeInTheDocument();
  fireEvent.click(screen.getByRole('button', { name: 'Analyze file' }));

  expect(
    await screen.findByRole('heading', {
      name: 'Choose the executable and review',
    }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole('option', { name: 'bin/game.exe' }),
  ).toBeInTheDocument();
  expect(screen.getByText('SHA-256 calculated')).toBeInTheDocument();

  fireEvent.click(screen.getByRole('button', { name: 'Upload and publish' }));
  expect(
    await screen.findByRole('heading', { name: 'Ready for players.' }),
  ).toBeInTheDocument();
  expect(screen.getByText('Published')).toBeInTheDocument();
  expect(publishing.publish).toHaveBeenCalledTimes(1);
});
