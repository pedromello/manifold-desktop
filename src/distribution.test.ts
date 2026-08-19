import { expect, it } from 'vitest';
import {
  createFixtureDistributionAdapter,
  distributionPlanSchema,
} from './distribution';

const releaseId = '11111111-1111-4111-8111-111111111111';
const artifactId = '22222222-2222-4222-8222-222222222222';
const checksum = 'a'.repeat(64);

const plan = {
  game_slug: 'capyvarias',
  release: {
    id: releaseId,
    version: '1.2.0',
    release_number: 2,
    published_at: '2026-08-19T12:00:00.000Z',
    artifact_id: artifactId,
    target: { platform: 'WINDOWS' as const, architecture: 'X86_64' as const },
    compressed_size_bytes: '1024',
    installed_size_bytes: '2048',
    sha256: checksum,
    manifest_schema_version: '1' as const,
  },
  manifest: {
    schema_version: '1' as const,
    release_id: releaseId,
    artifact_id: artifactId,
    entrypoint: 'Capyvarias.exe',
    launch_arguments: [],
    executables: ['Capyvarias.exe'],
    environment: {},
  },
  download: {
    artifact_id: artifactId,
    url: 'https://cdn.example.com/capyvarias.zip',
    expires_at: '2026-08-19T13:00:00.000Z',
    total_size_bytes: '1024',
    sha256: checksum,
  },
};

it('resolves a contract-compatible fixture through the distribution adapter', async () => {
  const adapter = createFixtureDistributionAdapter({ capyvarias: plan });
  await expect(adapter.resolve('capyvarias')).resolves.toEqual(plan);
});

it('rejects plans whose release, manifest, and download do not agree', () => {
  expect(() =>
    distributionPlanSchema.parse({
      ...plan,
      manifest: {
        ...plan.manifest,
        release_id: '33333333-3333-4333-8333-333333333333',
      },
    }),
  ).toThrow(/Manifest release/);
});

it('rejects manifest path traversal before native installation starts', () => {
  expect(() =>
    distributionPlanSchema.parse({
      ...plan,
      manifest: { ...plan.manifest, entrypoint: '../outside.exe' },
    }),
  ).toThrow(/traverse/);
});
