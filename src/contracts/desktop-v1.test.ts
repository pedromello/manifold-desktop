import { describe, expect, it } from 'vitest';
import {
  createSessionRequestSchema,
  desktopArchitectureSchema,
  desktopPlatformSchema,
  installManifestSchema,
} from './desktop-v1';

describe('Manifold Desktop API v1 contract', () => {
  it('uses the server target vocabulary', () => {
    expect(desktopPlatformSchema.parse('LINUX')).toBe('LINUX');
    expect(desktopArchitectureSchema.parse('X86_64')).toBe('X86_64');
    expect(() => desktopPlatformSchema.parse('linux')).toThrow();
  });

  it('requires clients to negotiate API v1 explicitly', () => {
    expect(
      createSessionRequestSchema.parse({
        method: 'PASSWORD',
        email: 'player@example.com',
        password: 'secret',
        api_version: '1',
      }).api_version,
    ).toBe('1');
    expect(() =>
      createSessionRequestSchema.parse({
        method: 'OTP',
        email: 'player@example.com',
        otp: '123456',
        api_version: '2',
      }),
    ).toThrow();
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
