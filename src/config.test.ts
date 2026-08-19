import { describe, expect, it } from 'vitest';
import { loadConfig } from './config';

describe('environment configuration', () => {
  it('loads an explicit environment', () =>
    expect(loadConfig('staging').environment).toBe('staging'));
  it('rejects unknown environments', () =>
    expect(() => loadConfig('preview')).toThrow('Invalid VITE_APP_ENV'));
});
