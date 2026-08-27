import { beforeEach, describe, expect, it } from 'vitest';
import {
  loadStoredPublisherReleases,
  parentDirectory,
  storePublisherRelease,
  StoredPublisherRelease,
} from './publishing';

function stored(
  phase: StoredPublisherRelease['phase'],
): StoredPublisherRelease {
  return {
    studioSlug: 'studio',
    gameSlug: 'game',
    gameTitle: 'Game',
    release: {
      id: 'release-1',
      gameId: 'game-1',
      version: '1.0.0',
      releaseNumber: 1,
      status: 'DRAFT',
      releaseNotes: null,
      publishedAt: null,
      createdAt: '2026-08-26T12:00:00.000Z',
      updatedAt: '2026-08-26T12:00:00.000Z',
    },
    archivePath: String.raw`C:\Games\game.zip`,
    inspection: null,
    manifest: null,
    phase,
    updatedAt: '2026-08-26T12:00:00.000Z',
  };
}

beforeEach(() => window.localStorage.clear());

describe('publisher recovery state', () => {
  it('persists draft data without upload authorization secrets', () => {
    storePublisherRelease(stored('file'));

    const [restored] = loadStoredPublisherReleases();
    expect(restored.release.id).toBe('release-1');
    expect(restored.archivePath).toBe(String.raw`C:\Games\game.zip`);
    expect(JSON.stringify(restored)).not.toContain('signed');
    expect(JSON.stringify(restored)).not.toContain('required_headers');
  });

  it.each(['preflight', 'uploading', 'verifying'] as const)(
    'returns interrupted %s work to a retryable step',
    (phase) => {
      storePublisherRelease({
        ...stored(phase),
        inspection: {
          archivePath: String.raw`C:\Games\game.zip`,
          fileName: 'game.zip',
          compressedSizeBytes: '10',
          installedSizeBytes: '20',
          sha256: '0'.repeat(64),
          executables: ['bin/game.exe'],
          suggestedEntrypoint: 'bin/game.exe',
          suggestedWorkingDirectory: 'bin',
        },
        manifest: {
          schemaVersion: '1',
          entrypoint: 'bin/game.exe',
          launchArguments: [],
          workingDirectory: 'bin',
          executables: ['bin/game.exe'],
          environment: {},
        },
      });

      expect(loadStoredPublisherReleases()[0].phase).toBe('manifest');
    },
  );

  it('derives a safe suggested working directory from the entrypoint', () => {
    expect(parentDirectory(String.raw`Peggy\Game.exe`)).toBe('Peggy');
    expect(parentDirectory('Game.exe')).toBeNull();
  });
});
