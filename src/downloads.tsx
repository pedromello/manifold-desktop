import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { distributionErrorKey } from './distribution-errors';
import { InstallationPhase, useInstallations } from './installation';

const activePhases: InstallationPhase[] = [
  'queued',
  'resolving',
  'downloading',
  'verifying',
  'extracting',
  'installing',
];

function humanBytes(value: number, locale: string) {
  if (!value) return '0 MB';
  const units = ['B', 'KB', 'MB', 'GB'];
  const index = Math.min(
    Math.floor(Math.log(value) / Math.log(1024)),
    units.length - 1,
  );
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(value / 1024 ** index)} ${units[index]}`;
}

function humanDuration(value: number, locale: string) {
  const format = new Intl.NumberFormat(locale, { maximumFractionDigits: 0 });
  if (value < 60) return `${format.format(Math.max(1, Math.ceil(value)))} s`;
  if (value < 3_600) return `${format.format(Math.ceil(value / 60))} min`;
  const hours = Math.floor(value / 3_600);
  const minutes = Math.ceil((value % 3_600) / 60);
  return minutes > 0
    ? `${format.format(hours)} h ${format.format(minutes)} min`
    : `${format.format(hours)} h`;
}

export function DownloadsPage() {
  const { t, i18n } = useTranslation();
  const { cancel, progress, install } = useInstallations();
  const jobs = Object.values(progress);
  const [cancelling, setCancelling] = useState<Record<string, boolean>>({});
  const [cancelErrors, setCancelErrors] = useState<Record<string, boolean>>({});

  async function cancelJob(gameSlug: string) {
    setCancelling((current) => ({ ...current, [gameSlug]: true }));
    setCancelErrors((current) => ({ ...current, [gameSlug]: false }));
    try {
      await cancel(gameSlug);
    } catch {
      setCancelErrors((current) => ({ ...current, [gameSlug]: true }));
      setCancelling((current) => ({ ...current, [gameSlug]: false }));
    }
  }

  return (
    <section className="downloads-page" aria-labelledby="downloads-title">
      <header className="page-header">
        <span className="eyebrow">{t('downloads.eyebrow')}</span>
        <h1 id="downloads-title">{t('downloads.title')}</h1>
        <p>{t('downloads.intro')}</p>
      </header>
      {jobs.length === 0 ? (
        <div className="catalog-state library-empty">
          <strong>{t('downloads.empty')}</strong>
          <span>{t('downloads.emptyHelp')}</span>
          <Link to="/library">{t('downloads.goLibrary')}</Link>
        </div>
      ) : (
        <div className="download-list">
          {jobs.map((job) => {
            const percentage = Math.round(
              (job.downloadedBytes / Math.max(job.totalBytes, 1)) * 100,
            );
            const active = activePhases.includes(job.phase);
            const statusId = `download-status-${job.gameSlug}`;
            return (
              <article
                aria-labelledby={`download-title-${job.gameSlug}`}
                className={`download-item phase-${job.phase}`}
                key={job.gameSlug}
              >
                <div className="download-icon">{job.title.slice(0, 1)}</div>
                <div className="download-content">
                  <div className="download-heading">
                    <div>
                      <h2 id={`download-title-${job.gameSlug}`}>{job.title}</h2>
                      <span aria-live="polite" id={statusId}>
                        {cancelling[job.gameSlug]
                          ? t('downloads.cancelling')
                          : t(`downloads.${job.phase}`)}
                      </span>
                    </div>
                    {job.version && <strong>v{job.version}</strong>}
                  </div>
                  {active && (
                    <>
                      <div
                        aria-label={t('downloads.progressLabel', {
                          title: job.title,
                        })}
                        aria-valuemax={100}
                        aria-valuemin={0}
                        aria-valuenow={percentage}
                        className="install-progress"
                        role="progressbar"
                      >
                        <span style={{ width: `${percentage}%` }} />
                      </div>
                      <div className="download-meta">
                        <span>{percentage}%</span>
                        {job.totalBytes > 0 && (
                          <span>
                            {t('downloads.bytes', {
                              done: humanBytes(
                                job.downloadedBytes,
                                i18n.language,
                              ),
                              total: humanBytes(job.totalBytes, i18n.language),
                            })}
                          </span>
                        )}
                        {job.phase === 'downloading' &&
                          job.bytesPerSecond &&
                          job.estimatedSecondsRemaining && (
                            <span>
                              {t('downloads.rate', {
                                rate: humanBytes(
                                  job.bytesPerSecond,
                                  i18n.language,
                                ),
                              })}{' '}
                              ·{' '}
                              {t('downloads.remaining', {
                                time: humanDuration(
                                  job.estimatedSecondsRemaining,
                                  i18n.language,
                                ),
                              })}
                            </span>
                          )}
                      </div>
                    </>
                  )}
                  {job.error && (
                    <p className="install-error" role="alert">
                      {t(distributionErrorKey(job.errorCode))}{' '}
                      {t(
                        job.retryable === false
                          ? 'downloads.nonRetryableHelp'
                          : 'downloads.failedHelp',
                      )}
                    </p>
                  )}
                  {job.phase === 'cancelled' && (
                    <p className="download-help">
                      {t('downloads.cancelledHelp')}
                    </p>
                  )}
                  {cancelErrors[job.gameSlug] && (
                    <p className="install-error" role="alert">
                      {t('downloads.cancelError')}
                    </p>
                  )}
                </div>
                {active ? (
                  <button
                    aria-describedby={statusId}
                    className="secondary-action"
                    disabled={cancelling[job.gameSlug]}
                    onClick={() => void cancelJob(job.gameSlug)}
                  >
                    {cancelling[job.gameSlug]
                      ? t('downloads.cancelling')
                      : t('downloads.cancel')}
                  </button>
                ) : job.phase === 'failed' || job.phase === 'cancelled' ? (
                  <button
                    className="secondary-action"
                    onClick={() => void install(job.gameSlug, job.title)}
                  >
                    {t('downloads.retry')}
                  </button>
                ) : null}
              </article>
            );
          })}
        </div>
      )}
    </section>
  );
}
