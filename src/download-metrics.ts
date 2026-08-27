const MIN_SAMPLE_DURATION_MS = 750;
const STALLED_SAMPLE_DURATION_MS = 5_000;
const SMOOTHING_WEIGHT = 0.25;

export type DownloadMetricState = {
  sampledAtMs: number;
  sampledBytes: number;
  bytesPerSecond?: number;
};

export type DownloadMetrics = {
  state: DownloadMetricState;
  bytesPerSecond?: number;
  estimatedSecondsRemaining?: number;
};

function result(
  state: DownloadMetricState,
  downloadedBytes: number,
  totalBytes: number,
): DownloadMetrics {
  const bytesPerSecond = state.bytesPerSecond;
  if (!bytesPerSecond || bytesPerSecond <= 0 || downloadedBytes >= totalBytes) {
    return { state, bytesPerSecond };
  }
  return {
    state,
    bytesPerSecond,
    estimatedSecondsRemaining: Math.ceil(
      (totalBytes - downloadedBytes) / bytesPerSecond,
    ),
  };
}

export function updateDownloadMetrics(
  previous: DownloadMetricState | undefined,
  downloadedBytes: number,
  totalBytes: number,
  nowMs: number,
): DownloadMetrics {
  if (!previous || downloadedBytes < previous.sampledBytes || totalBytes <= 0) {
    return result(
      { sampledAtMs: nowMs, sampledBytes: downloadedBytes },
      downloadedBytes,
      totalBytes,
    );
  }

  const elapsedMs = nowMs - previous.sampledAtMs;
  const downloadedSinceSample = downloadedBytes - previous.sampledBytes;
  if (elapsedMs < MIN_SAMPLE_DURATION_MS) {
    return result(previous, downloadedBytes, totalBytes);
  }
  if (downloadedSinceSample <= 0) {
    if (elapsedMs < STALLED_SAMPLE_DURATION_MS) {
      return result(previous, downloadedBytes, totalBytes);
    }
    return result(
      { sampledAtMs: nowMs, sampledBytes: downloadedBytes },
      downloadedBytes,
      totalBytes,
    );
  }

  const instantaneousRate = (downloadedSinceSample * 1_000) / elapsedMs;
  const bytesPerSecond = previous.bytesPerSecond
    ? previous.bytesPerSecond * (1 - SMOOTHING_WEIGHT) +
      instantaneousRate * SMOOTHING_WEIGHT
    : instantaneousRate;
  return result(
    { sampledAtMs: nowMs, sampledBytes: downloadedBytes, bytesPerSecond },
    downloadedBytes,
    totalBytes,
  );
}
