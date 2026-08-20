import {
  desktopErrorCodeSchema,
  type DesktopErrorCode,
} from './contracts/desktop-v1';

export type DistributionErrorCode = DesktopErrorCode | 'INSTALLATION_FAILED';

export type DistributionFailure = {
  code: DistributionErrorCode;
  message: string;
  retryable: boolean;
};

function candidate(reason: unknown): unknown {
  if (typeof reason !== 'string') return reason;
  try {
    return JSON.parse(reason);
  } catch {
    return reason;
  }
}

export function normalizeDistributionFailure(
  reason: unknown,
): DistributionFailure {
  const value = candidate(reason);
  if (value && typeof value === 'object') {
    const error = value as Record<string, unknown>;
    const code = desktopErrorCodeSchema.safeParse(error.code);
    if (
      code.success &&
      typeof error.message === 'string' &&
      typeof error.retryable === 'boolean'
    ) {
      return {
        code: code.data,
        message: error.message,
        retryable: error.retryable,
      };
    }
  }
  return {
    code: 'INSTALLATION_FAILED',
    message:
      typeof reason === 'string'
        ? reason
        : reason instanceof Error
          ? reason.message
          : 'Installation failed',
    retryable: true,
  };
}

export function distributionErrorKey(code?: DistributionErrorCode) {
  switch (code) {
    case 'AUTHENTICATION_REQUIRED':
    case 'INVALID_CREDENTIALS':
    case 'SESSION_EXPIRED':
    case 'SESSION_REVOKED':
      return 'errors.sessionExpired';
    case 'ACCOUNT_DISABLED':
      return 'errors.accountDisabled';
    case 'ENTITLEMENT_REQUIRED':
      return 'errors.entitlementRequired';
    case 'NO_COMPATIBLE_RELEASE':
      return 'errors.distributionUnavailable';
    case 'RELEASE_RETIRED':
      return 'errors.releaseRetired';
    case 'INTEGRITY_FAILURE':
      return 'errors.integrityFailure';
    case 'RATE_LIMITED':
      return 'errors.rateLimited';
    case 'SERVICE_UNAVAILABLE':
      return 'errors.serviceUnavailable';
    case 'INVALID_REQUEST':
    case 'UNSUPPORTED_API_VERSION':
    case 'UNSUPPORTED_MANIFEST_VERSION':
      return 'errors.incompatibleResponse';
    default:
      return 'errors.installFailed';
  }
}
