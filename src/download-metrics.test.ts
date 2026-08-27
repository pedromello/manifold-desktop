import { expect, it } from 'vitest';
import { updateDownloadMetrics } from './download-metrics';

it('waits for a stable sample before estimating speed and remaining time', () => {
  const initial = updateDownloadMetrics(undefined, 0, 10_000, 0);
  const early = updateDownloadMetrics(initial.state, 1_000, 10_000, 500);
  const measured = updateDownloadMetrics(early.state, 2_000, 10_000, 1_000);

  expect(early.bytesPerSecond).toBeUndefined();
  expect(measured.bytesPerSecond).toBe(2_000);
  expect(measured.estimatedSecondsRemaining).toBe(4);
});

it('smooths later samples and preserves a monotonic remaining estimate', () => {
  const first = updateDownloadMetrics(undefined, 0, 10_000, 0);
  const measured = updateDownloadMetrics(first.state, 2_000, 10_000, 1_000);
  const smoothed = updateDownloadMetrics(measured.state, 3_000, 10_000, 2_000);

  expect(smoothed.bytesPerSecond).toBe(1_750);
  expect(smoothed.estimatedSecondsRemaining).toBe(4);
});

it('hides stale estimates after a prolonged period without progress', () => {
  const first = updateDownloadMetrics(undefined, 0, 10_000, 0);
  const measured = updateDownloadMetrics(first.state, 2_000, 10_000, 1_000);
  const stalled = updateDownloadMetrics(measured.state, 2_000, 10_000, 6_500);

  expect(stalled.bytesPerSecond).toBeUndefined();
  expect(stalled.estimatedSecondsRemaining).toBeUndefined();
});

it('starts a fresh sample when a download restarts from a lower offset', () => {
  const first = updateDownloadMetrics(undefined, 5_000, 10_000, 0);
  const restarted = updateDownloadMetrics(first.state, 0, 10_000, 1_000);

  expect(restarted.state.sampledBytes).toBe(0);
  expect(restarted.bytesPerSecond).toBeUndefined();
});
