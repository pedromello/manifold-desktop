import { expect, it } from 'vitest';
import type { ReleaseSummary } from './contracts/desktop-v1';
import { InstalledGame, isUpdateAvailable } from './installation';

const installed: InstalledGame = {
  gameSlug: 'capyvarias',
  title: 'Capyvarias',
  version: '1.0.0',
  releaseId: '11111111-1111-4111-8111-111111111111',
  releaseNumber: 1,
  artifactId: '22222222-2222-4222-8222-222222222222',
  installedSizeBytes: '2048',
  installDirectory: 'C:\\Games\\Capyvarias',
  entrypoint: 'Capyvarias.exe',
  installedAt: '2026-08-19T12:00:00.000Z',
  status: 'INSTALLED',
};

const latest: ReleaseSummary = {
  id: '33333333-3333-4333-8333-333333333333',
  version: '2.0.0',
  release_number: 2,
  published_at: '2026-08-19T13:00:00.000Z',
  artifact_id: '44444444-4444-4444-8444-444444444444',
  target: { platform: 'WINDOWS', architecture: 'X86_64' },
  compressed_size_bytes: '1024',
  installed_size_bytes: '4096',
  sha256: 'a'.repeat(64),
  manifest_schema_version: '1',
};

it('exposes an update only for a newer monotonic release', () => {
  expect(isUpdateAvailable(installed, latest)).toBe(true);
  expect(
    isUpdateAvailable(installed, {
      ...latest,
      id: '55555555-5555-4555-8555-555555555555',
      release_number: 1,
    }),
  ).toBe(false);
});

it('does not replace repair with an update action', () => {
  expect(
    isUpdateAvailable({ ...installed, status: 'REPAIR_NEEDED' }, latest),
  ).toBe(false);
});

it('does not treat the same release as an update', () => {
  expect(
    isUpdateAvailable(installed, {
      ...latest,
      id: installed.releaseId,
    }),
  ).toBe(false);
});
