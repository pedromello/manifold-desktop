import type { InstalledGame } from './installation';
import type { DistributionAdapter, DistributionPlan } from './distribution';
import { normalizeDistributionFailure } from './distribution-errors';

const AUTHORIZATION_SAFETY_WINDOW_MS = 60_000;
const MAX_AUTHORIZATION_REFRESHES = 2;

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
  let refreshes = 0;
  let plan = await adapter.resolve(gameSlug);

  while (authorizationExpiresSoon(plan, now())) {
    if (refreshes >= MAX_AUTHORIZATION_REFRESHES) {
      throw {
        code: 'DOWNLOAD_AUTHORIZATION_EXPIRED',
        message: 'download authorization expired before the transfer started',
        retryable: true,
      };
    }
    refreshes += 1;
    plan = await adapter.resolve(gameSlug);
  }

  while (true) {
    try {
      return await install(title, plan);
    } catch (reason) {
      const failure = normalizeDistributionFailure(reason);
      if (
        failure.code !== 'DOWNLOAD_AUTHORIZATION_EXPIRED' ||
        refreshes >= MAX_AUTHORIZATION_REFRESHES
      ) {
        throw reason;
      }
      refreshes += 1;
      plan = await adapter.resolve(gameSlug);
    }
  }
}
