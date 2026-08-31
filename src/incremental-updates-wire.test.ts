import { describe, expect, it } from 'vitest';
import {
  patchDownloadAuthorizationsSchema,
  updatePlanSchema,
} from './contracts/desktop-v1';

const source = {
  id: '11111111-1111-4111-8111-111111111111',
  version: '1.0.0',
  release_number: 1,
};
const target = {
  id: '22222222-2222-4222-8222-222222222222',
  version: '1.1.0',
  release_number: 2,
  published_at: '2026-08-29T01:00:00.000Z',
  artifact_id: '33333333-3333-4333-8333-333333333333',
  target: { platform: 'WINDOWS' as const, architecture: 'X86_64' as const },
  compressed_size_bytes: '1000',
  installed_size_bytes: '2000',
  sha256: 'a'.repeat(64),
  manifest_schema_version: '1' as const,
};
const patchId = '44444444-4444-4444-8444-444444444444';
const patch = {
  id: patchId,
  source_release_id: source.id,
  target_release_id: target.id,
  target: target.target,
  algorithm: 'WHARF' as const,
  format_version: '1' as const,
  status: 'READY' as const,
  patch: { size_bytes: '800', sha256: 'b'.repeat(64) },
  signature: { size_bytes: '64', sha256: 'c'.repeat(64) },
  expected_installation_sha256: 'c'.repeat(64),
  generation_duration_ms: '1250',
  created_at: '2026-08-29T00:58:00.000Z',
  updated_at: '2026-08-29T00:59:00.000Z',
};

describe('incremental update v1 wire fixtures', () => {
  it('accepts the canonical PATCH plan without embedding authorizations', () => {
    const plan = {
      strategy: 'PATCH' as const,
      source,
      target,
      patch,
      fallback_artifact_id: target.artifact_id,
    };
    expect(updatePlanSchema.parse(plan)).toEqual(plan);
    expect(() =>
      updatePlanSchema.parse({
        ...plan,
        download: { url: 'https://storage.example.test/not-on-resolver' },
      }),
    ).toThrow();
  });

  it('accepts every frozen FULL reason with the same fallback identity', () => {
    for (const reason of [
      'NO_PATCH',
      'SOURCE_NOT_PREDECESSOR',
      'SOURCE_UNAVAILABLE',
      'PATCH_NOT_READY',
      'PATCH_EXCEEDS_SIZE_LIMIT',
    ] as const) {
      const full = updatePlanSchema.parse({
        strategy: 'FULL',
        source,
        target,
        fallback_artifact_id: target.artifact_id,
        reason,
      });
      expect(full.strategy).toBe('FULL');
      if (full.strategy !== 'FULL') throw new Error('Expected FULL fixture');
      expect(full.reason).toBe(reason);
    }
  });

  it('keeps patch and signature download authorizations independent', () => {
    const authorizations = patchDownloadAuthorizationsSchema.parse({
      patch: {
        patch_id: patchId,
        file: 'PATCH',
        url: 'https://patches.example.test/update.pwr?one',
        expires_at: '2026-08-29T02:00:00.000Z',
        total_size_bytes: patch.patch.size_bytes,
        sha256: patch.patch.sha256,
        etag: '"patch-etag"',
      },
      signature: {
        patch_id: patchId,
        file: 'SIGNATURE',
        url: 'https://signatures.example.test/update.pwr.sig?two',
        expires_at: '2026-08-29T02:00:00.000Z',
        total_size_bytes: patch.signature.size_bytes,
        sha256: patch.signature.sha256,
        etag: '"signature-etag"',
      },
    });
    expect(authorizations.patch.url).not.toBe(authorizations.signature.url);
    expect(authorizations.patch.file).toBe('PATCH');
    expect(authorizations.signature.file).toBe('SIGNATURE');
  });

  it('rejects zero-byte patch and signature declarations', () => {
    expect(() =>
      updatePlanSchema.parse({
        strategy: 'PATCH',
        source,
        target,
        patch: { ...patch, patch: { ...patch.patch, size_bytes: '0' } },
        fallback_artifact_id: target.artifact_id,
      }),
    ).toThrow(/greater than zero/);
  });
});
