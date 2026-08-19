import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import {
  downloadAuthorizationSchema,
  installManifestSchema,
  releaseSummarySchema,
} from './contracts/desktop-v1';
import type { ReleaseSummary } from './contracts/desktop-v1';

export const distributionPlanSchema = z
  .object({
    game_slug: z.string().min(1),
    release: releaseSummarySchema,
    manifest: installManifestSchema,
    download: downloadAuthorizationSchema,
  })
  .superRefine((plan, context) => {
    if (plan.release.id !== plan.manifest.release_id) {
      context.addIssue({
        code: 'custom',
        message: 'Manifest release does not match the resolved release',
      });
    }
    if (
      plan.release.artifact_id !== plan.manifest.artifact_id ||
      plan.release.artifact_id !== plan.download.artifact_id
    ) {
      context.addIssue({
        code: 'custom',
        message: 'Artifact identifiers do not match',
      });
    }
    if (plan.release.sha256 !== plan.download.sha256) {
      context.addIssue({
        code: 'custom',
        message: 'Artifact checksums do not match',
      });
    }
  });

export type DistributionPlan = z.infer<typeof distributionPlanSchema>;

export interface DistributionAdapter {
  latest(gameSlug: string): Promise<ReleaseSummary>;
  resolve(gameSlug: string): Promise<DistributionPlan>;
}

export const productionDistributionAdapter: DistributionAdapter = {
  async latest(gameSlug) {
    const response = await invoke<unknown>('resolve_latest_release', {
      gameSlug,
    });
    return releaseSummarySchema.parse(response);
  },
  async resolve(gameSlug) {
    const response = await invoke<unknown>('resolve_install_plan', {
      gameSlug,
    });
    return distributionPlanSchema.parse(response);
  },
};

export function createFixtureDistributionAdapter(
  plans: Record<string, DistributionPlan>,
): DistributionAdapter {
  return {
    async latest(gameSlug) {
      const plan = plans[gameSlug];
      if (!plan) throw new Error(`No distribution fixture for ${gameSlug}`);
      return releaseSummarySchema.parse(plan.release);
    },
    async resolve(gameSlug) {
      const plan = plans[gameSlug];
      if (!plan) throw new Error(`No distribution fixture for ${gameSlug}`);
      return distributionPlanSchema.parse(plan);
    },
  };
}
