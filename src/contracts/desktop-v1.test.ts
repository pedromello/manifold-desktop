import { describe, expect, it } from 'vitest';
import {
  createSessionRequestSchema,
  desktopApiVersionSchema,
  desktopArchitectureSchema,
  desktopPlatformSchema,
  installManifestSchema,
  libraryItemSchema,
  requestOtpSchema,
} from './desktop-v1';

describe('Manifold distribution API v1 contract', () => {
  it('uses the server target vocabulary', () => {
    expect(desktopPlatformSchema.parse('LINUX')).toBe('LINUX');
    expect(desktopArchitectureSchema.parse('X86_64')).toBe('X86_64');
    expect(() => desktopPlatformSchema.parse('linux')).toThrow();
  });

  it('requires clients to negotiate API v1 explicitly', () => {
    expect(desktopApiVersionSchema.parse('1')).toBe('1');
    expect(() => desktopApiVersionSchema.parse('2')).toThrow();
  });

  it('defines a passwordless OTP login flow', () => {
    expect(
      requestOtpSchema.safeParse({
        login: 'player@example.com',
      }).success,
    ).toBe(true);
    expect(
      createSessionRequestSchema.safeParse({
        login: 'player@example.com',
        code: '123456',
      }).success,
    ).toBe(true);
    expect(
      createSessionRequestSchema.safeParse({
        login: 'player@example.com',
        code: 'password',
      }).success,
    ).toBe(false);
  });

  it('accepts a library item with a compatible release', () => {
    expect(
      libraryItemSchema.safeParse({
        game: {
          id: 'e8b521ce-ed36-4d5a-86ab-bc96e151a504',
          slug: 'strategos-void',
          title: 'Strategos Void',
          description: '',
          cover_url: null,
          platforms: ['WINDOWS'],
        },
        acquired_at: '2026-08-28T12:00:00.000Z',
        latest_compatible_release: null,
      }).success,
    ).toBe(true);
  });
  it('rejects install paths that escape the installation root', () => {
    const manifest = {
      schema_version: '1',
      release_id: '3a63848e-54f9-48ce-9f32-b6e25fa89f8d',
      artifact_id: '762eeae6-e356-45ba-92a6-c45dac8e2810',
      entrypoint: '../game',
    };
    expect(() => installManifestSchema.parse(manifest)).toThrow();
  });
});
