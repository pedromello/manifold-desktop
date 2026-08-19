import { useTranslation } from 'react-i18next';
import { Link } from 'react-router-dom';
import { InstallationPhase, useInstallations } from './installation';

const activePhases: InstallationPhase[] = [
  'queued',
  'resolving',
  'downloading',
  'verifying',
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

export function DownloadsPage() {
  const { t, i18n } = useTranslation();
  const { cancel, progress, install } = useInstallations();
  const jobs = Object.values(progress);

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
            return (
              <article className="download-item" key={job.gameSlug}>
                <div className="download-icon">{job.title.slice(0, 1)}</div>
                <div className="download-content">
                  <div className="download-heading">
                    <div>
                      <h2>{job.title}</h2>
                      <span>{t(`downloads.${job.phase}`)}</span>
                    </div>
                    {job.version && <strong>v{job.version}</strong>}
                  </div>
                  {active && (
                    <>
                      <div className="install-progress">
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
                      </div>
                    </>
                  )}
                  {job.error && (
                    <p className="install-error" role="alert">
                      {job.error}
                    </p>
                  )}
                </div>
                {active ? (
                  <button
                    className="secondary-action"
                    onClick={() => void cancel(job.gameSlug)}
                  >
                    {t('downloads.cancel')}
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
