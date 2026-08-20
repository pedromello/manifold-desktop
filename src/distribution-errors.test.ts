import { expect, it } from 'vitest';
import {
  distributionErrorKey,
  normalizeDistributionFailure,
} from './distribution-errors';

it('preserves structured API failures without exposing their message directly', () => {
  expect(
    normalizeDistributionFailure({
      code: 'ENTITLEMENT_REQUIRED',
      message: 'Acquire the game',
      retryable: false,
    }),
  ).toEqual({
    code: 'ENTITLEMENT_REQUIRED',
    message: 'Acquire the game',
    retryable: false,
  });
  expect(distributionErrorKey('ENTITLEMENT_REQUIRED')).toBe(
    'errors.entitlementRequired',
  );
});

it('accepts serialized Tauri errors and safely classifies unknown failures', () => {
  expect(
    normalizeDistributionFailure(
      JSON.stringify({
        code: 'RATE_LIMITED',
        message: 'Slow down',
        retryable: true,
      }),
    ).code,
  ).toBe('RATE_LIMITED');
  expect(normalizeDistributionFailure(new Error('native failure'))).toEqual({
    code: 'INSTALLATION_FAILED',
    message: 'native failure',
    retryable: true,
  });
});
