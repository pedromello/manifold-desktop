import { invoke } from '@tauri-apps/api/core';
import { z } from 'zod';
import {
  downloadAuthorizationSchema,
  installManifestSchema,
  patchDownloadAuthorizationsSchema,
  releaseSummarySchema,
  updatePlanSchema,
} from './contracts/desktop-v1';
import type { ReleaseSummary, UpdatePlan } from './contracts/desktop-v1';

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

export const updateExecutionPlanSchema = z
  .object({
    update: updatePlanSchema,
    manifest: installManifestSchema,
    patch_downloads: patchDownloadAuthorizationsSchema.nullable(),
    fallback_download: downloadAuthorizationSchema,
  })
  .strict();

export type UpdateExecutionPlan = z.infer<typeof updateExecutionPlanSchema>;

export interface DistributionAdapter {
  latest(gameSlug: string): Promise<ReleaseSummary>;
  resolve(gameSlug: string): Promise<DistributionPlan>;
  resolveUpdate(
    gameSlug: string,
    installedReleaseId: string,
  ): Promise<UpdatePlan>;
  prepareUpdate(plan: UpdatePlan): Promise<UpdateExecutionPlan>;
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
  async resolveUpdate(gameSlug, installedReleaseId) {
    const response = await invoke<unknown>('resolve_update_plan', {
      gameSlug,
      sourceReleaseId: installedReleaseId,
    });
    return updatePlanSchema.parse(response);
  },
  async prepareUpdate(update) {
    const response = await invoke<unknown>('prepare_update_plan', { update });
    return updateExecutionPlanSchema.parse(response);
  },
};

export function createFixtureDistributionAdapter(
  plans: Record<string, DistributionPlan>,
  updates: Record<string, UpdatePlan> = {},
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
    async resolveUpdate(gameSlug, installedReleaseId) {
      const plan = updates[gameSlug];
      if (!plan) throw new Error(`No update fixture for ${gameSlug}`);
      if (
        plan.strategy === 'PATCH' &&
        plan.patch.source_release_id !== installedReleaseId
      ) {
        throw new Error(
          `Update fixture does not start at ${installedReleaseId}`,
        );
      }
      return updatePlanSchema.parse(plan);
    },
    async prepareUpdate() {
      throw new Error('No hydrated update fixture was provided');
    },
  };
}
