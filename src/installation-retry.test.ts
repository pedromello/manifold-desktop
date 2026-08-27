import { expect, it, vi } from 'vitest';
import type { DistributionPlan } from './distribution';
import {
  authorizationExpiresSoon,
  installWithAuthorizationRefresh,
} from './installation-retry';

const plan = {
  game_slug: 'capyvarias',
  release: {
    id: '11111111-1111-4111-8111-111111111111',
    version: '1.0.0',
    release_number: 1,
    published_at: '2026-08-20T20:00:00.000Z',
    artifact_id: '22222222-2222-4222-8222-222222222222',
    target: { platform: 'WINDOWS', architecture: 'X86_64' },
    compressed_size_bytes: '8',
    installed_size_bytes: '8',
    sha256: 'a'.repeat(64),
    manifest_schema_version: '1',
  },
  manifest: {
    schema_version: '1',
    release_id: '11111111-1111-4111-8111-111111111111',
    artifact_id: '22222222-2222-4222-8222-222222222222',
    entrypoint: 'game.exe',
    launch_arguments: [],
    executables: ['game.exe'],
    environment: {},
  },
  download: {
    artifact_id: '22222222-2222-4222-8222-222222222222',
    url: 'https://downloads.example.test/artifact.zip',
    expires_at: '2026-08-20T21:00:00.000Z',
    total_size_bytes: '8',
    sha256: 'a'.repeat(64),
    etag: 'artifact-v1',
  },
} satisfies DistributionPlan;

it('treats authorizations inside the safety window as expiring', () => {
  expect(
    authorizationExpiresSoon(plan, Date.parse('2026-08-20T20:59:30.000Z')),
  ).toBe(true);
  expect(
    authorizationExpiresSoon(plan, Date.parse('2026-08-20T20:58:59.000Z')),
  ).toBe(false);
});

it('renews an authorization that will expire before a download can safely start', async () => {
  const fresh = {
    ...plan,
    download: { ...plan.download, expires_at: '2026-08-20T22:00:00.000Z' },
  };
  const resolve = vi.fn().mockResolvedValueOnce(plan).mockResolvedValue(fresh);
  const install = vi.fn().mockResolvedValue({ gameSlug: 'capyvarias' });

  await installWithAuthorizationRefresh(
    { resolve, latest: vi.fn() },
    'capyvarias',
    'Capyvarias',
    install,
    () => Date.parse('2026-08-20T20:59:30.000Z'),
  );

  expect(resolve).toHaveBeenCalledTimes(2);
  expect(install).toHaveBeenCalledWith('Capyvarias', fresh);
});

it('renews after storage rejects an expired URL and retries the native install', async () => {
  const fresh = {
    ...plan,
    download: { ...plan.download, url: 'https://downloads.example.test/fresh' },
  };
  const resolve = vi.fn().mockResolvedValueOnce(plan).mockResolvedValue(fresh);
  const install = vi
    .fn()
    .mockRejectedValueOnce({
      code: 'DOWNLOAD_AUTHORIZATION_EXPIRED',
      message: 'expired',
      retryable: true,
    })
    .mockResolvedValueOnce({ gameSlug: 'capyvarias' });

  await installWithAuthorizationRefresh(
    { resolve, latest: vi.fn() },
    'capyvarias',
    'Capyvarias',
    install,
    () => Date.parse('2026-08-20T20:00:00.000Z'),
  );

  expect(resolve).toHaveBeenCalledTimes(2);
  expect(install).toHaveBeenNthCalledWith(2, 'Capyvarias', fresh);
});

it('transparently obtains a fresh authorization after native network recovery is exhausted', async () => {
  const fresh = {
    ...plan,
    download: { ...plan.download, url: 'https://downloads.example.test/fresh' },
  };
  const resolve = vi.fn().mockResolvedValueOnce(plan).mockResolvedValue(fresh);
  const install = vi
    .fn()
    .mockRejectedValueOnce({
      code: 'DOWNLOAD_INTERRUPTED',
      message: 'interrupted',
      retryable: true,
    })
    .mockResolvedValueOnce({ gameSlug: 'capyvarias' });

  await installWithAuthorizationRefresh(
    { resolve, latest: vi.fn() },
    'capyvarias',
    'Capyvarias',
    install,
    () => Date.parse('2026-08-20T20:00:00.000Z'),
  );

  expect(resolve).toHaveBeenCalledTimes(2);
  expect(install).toHaveBeenNthCalledWith(2, 'Capyvarias', fresh);
});

it('bounds consecutive automatic recoveries when no attempt succeeds', async () => {
  const failure = {
    code: 'DOWNLOAD_INTERRUPTED',
    message: 'interrupted',
    retryable: true,
  };
  const resolve = vi.fn().mockResolvedValue(plan);
  const install = vi.fn().mockRejectedValue(failure);

  await expect(
    installWithAuthorizationRefresh(
      { resolve, latest: vi.fn() },
      'capyvarias',
      'Capyvarias',
      install,
      () => Date.parse('2026-08-20T20:00:00.000Z'),
    ),
  ).rejects.toEqual(failure);

  expect(resolve).toHaveBeenCalledTimes(9);
  expect(install).toHaveBeenCalledTimes(9);
});
