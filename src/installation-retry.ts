import type { InstalledGame } from './installation';
import type { DistributionAdapter, DistributionPlan } from './distribution';
import { normalizeDistributionFailure } from './distribution-errors';

const AUTHORIZATION_SAFETY_WINDOW_MS = 60_000;
const MAX_DOWNLOAD_RECOVERIES = 8;
const recoverableDownloadCodes = new Set([
  'DOWNLOAD_AUTHORIZATION_EXPIRED',
  'DOWNLOAD_INTERRUPTED',
]);

export type InstallInvoker = (
  title: string,
  plan: DistributionPlan,
) => Promise<InstalledGame>;

export function authorizationExpiresSoon(
  plan: DistributionPlan,
  now = Date.now(),
) {
  return (
    Date.parse(plan.download.expires_at) - now <= AUTHORIZATION_SAFETY_WINDOW_MS
  );
}

export async function installWithAuthorizationRefresh(
  adapter: DistributionAdapter,
  gameSlug: string,
  title: string,
  install: InstallInvoker,
  now = () => Date.now(),
) {
  let recoveries = 0;
  let plan = await adapter.resolve(gameSlug);

  while (authorizationExpiresSoon(plan, now())) {
    if (recoveries >= MAX_DOWNLOAD_RECOVERIES) {
      throw {
        code: 'DOWNLOAD_AUTHORIZATION_EXPIRED',
        message: 'download authorization expired before the transfer started',
        retryable: true,
      };
    }
    recoveries += 1;
    plan = await adapter.resolve(gameSlug);
  }

  while (true) {
    try {
      return await install(title, plan);
    } catch (reason) {
      const failure = normalizeDistributionFailure(reason);
      if (
        !recoverableDownloadCodes.has(failure.code) ||
        recoveries >= MAX_DOWNLOAD_RECOVERIES
      ) {
        throw reason;
      }
      recoveries += 1;
      plan = await adapter.resolve(gameSlug);
    }
  }
}
