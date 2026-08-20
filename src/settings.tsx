import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { FormEvent, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { InstallationPreferences } from './installation';

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const [preferences, setPreferences] =
    useState<InstallationPreferences | null>(null);
  const [directory, setDirectory] = useState('');
  const [saved, setSaved] = useState(false);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [errorKey, setErrorKey] = useState<string | null>(null);

  useEffect(() => {
    invoke<InstallationPreferences>('get_installation_preferences')
      .then((value) => {
        setPreferences(value);
        setDirectory(value.installDirectory ?? '');
      })
      .catch(() => {
        setPreferences(null);
        setErrorKey('settings.loadError');
      })
      .finally(() => setLoading(false));
  }, []);

  async function save(event: FormEvent) {
    event.preventDefault();
    setSaving(true);
    setSaved(false);
    setErrorKey(null);
    try {
      const value = await invoke<InstallationPreferences>(
        'set_installation_preferences',
        {
          installDirectory: directory.trim() || null,
        },
      );
      setPreferences(value);
      setDirectory(value.installDirectory ?? '');
      setSaved(true);
      window.setTimeout(() => setSaved(false), 2500);
    } catch {
      setErrorKey('settings.saveError');
    } finally {
      setSaving(false);
    }
  }

  async function changeLanguage(language: string) {
    await i18n.changeLanguage(language);
  }

  async function chooseDirectory() {
    setErrorKey(null);
    try {
      const selected = await open({ directory: true, multiple: false });
      if (typeof selected === 'string') setDirectory(selected);
    } catch {
      setErrorKey('settings.folderError');
    }
  }

  return (
    <section className="settings-page" aria-labelledby="settings-title">
      <header className="page-header">
        <span className="eyebrow">{t('settings.eyebrow')}</span>
        <h1 id="settings-title">{t('settings.title')}</h1>
      </header>
      <div className="settings-card">
        <label>
          <strong>{t('settings.language')}</strong>
          <span id="language-help">{t('settings.languageHelp')}</span>
          <select
            aria-describedby="language-help"
            value={i18n.language}
            onChange={(event) => void changeLanguage(event.target.value)}
          >
            <option value="pt-BR">{t('settings.portuguese')}</option>
            <option value="en-US">{t('settings.english')}</option>
          </select>
        </label>
      </div>
      <form className="settings-card" onSubmit={save}>
        <label>
          <strong>{t('settings.installLocation')}</strong>
          <span id="install-help">{t('settings.installHelp')}</span>
          <input
            aria-describedby="install-help install-location-status"
            aria-label={t('settings.installLocation')}
            disabled={loading || saving}
            placeholder={preferences?.defaultInstallDirectory ?? ''}
            value={directory}
            onChange={(event) => setDirectory(event.target.value)}
          />
          <button
            className="secondary-action folder-picker"
            disabled={loading || saving}
            type="button"
            onClick={() => void chooseDirectory()}
          >
            {t('settings.chooseFolder')}
          </button>
          <small id="install-location-status">
            {loading
              ? t('settings.loading')
              : directory
                ? t('settings.customLocation')
                : t('settings.defaultLocation')}
          </small>
        </label>
        <button
          className="game-action"
          disabled={loading || saving}
          type="submit"
        >
          {saving ? t('settings.saving') : t('settings.save')}
        </button>
        {saved && (
          <span className="settings-saved" role="status">
            {t('settings.saved')}
          </span>
        )}
        {errorKey && (
          <span className="settings-error" role="alert">
            {t(errorKey)}
          </span>
        )}
      </form>
    </section>
  );
}
