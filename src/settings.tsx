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

  useEffect(() => {
    invoke<InstallationPreferences>('get_installation_preferences')
      .then((value) => {
        setPreferences(value);
        setDirectory(value.installDirectory ?? '');
      })
      .catch(() => setPreferences(null));
  }, []);

  async function save(event: FormEvent) {
    event.preventDefault();
    const value = await invoke<InstallationPreferences>(
      'set_installation_preferences',
      {
        installDirectory: directory.trim() || null,
      },
    );
    setPreferences(value);
    setSaved(true);
    window.setTimeout(() => setSaved(false), 2500);
  }

  async function changeLanguage(language: string) {
    await i18n.changeLanguage(language);
  }

  async function chooseDirectory() {
    const selected = await open({ directory: true, multiple: false });
    if (typeof selected === 'string') setDirectory(selected);
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
          <span>{t('settings.languageHelp')}</span>
          <select
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
          <span>{t('settings.installHelp')}</span>
          <input
            placeholder={preferences?.defaultInstallDirectory ?? ''}
            value={directory}
            onChange={(event) => setDirectory(event.target.value)}
          />
          <button
            className="secondary-action folder-picker"
            type="button"
            onClick={() => void chooseDirectory()}
          >
            {t('settings.chooseFolder')}
          </button>
          {!directory && <small>{t('settings.defaultLocation')}</small>}
        </label>
        <button className="game-action" type="submit">
          {t('settings.save')}
        </button>
        {saved && (
          <span className="settings-saved" role="status">
            {t('settings.saved')}
          </span>
        )}
      </form>
    </section>
  );
}
