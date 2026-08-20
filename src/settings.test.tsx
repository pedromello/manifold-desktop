import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { SettingsPage } from './settings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

const writeText = vi.fn();
Object.defineProperty(navigator, 'clipboard', {
  configurable: true,
  value: { writeText },
});

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
  vi.mocked(open).mockReset();
  writeText.mockReset();
});

it('loads and copies sanitized installation diagnostics', async () => {
  const diagnostics = {
    appVersion: '0.1.0',
    events: [
      {
        timestamp: 1776000000,
        gameSlug: 'capyvarias',
        event: 'FAILED',
        releaseId: 'release-1',
        artifactId: 'artifact-1',
        version: '1.0.0',
        totalBytes: '1024',
        errorCode: 'DOWNLOAD_FAILED',
      },
    ],
  };
  vi.mocked(invoke)
    .mockResolvedValueOnce({
      installDirectory: null,
      defaultInstallDirectory: 'C:\\Games\\Manifold',
    })
    .mockResolvedValueOnce(diagnostics);
  writeText.mockResolvedValueOnce(undefined);

  render(<SettingsPage />);
  await screen.findByRole('textbox', { name: 'Installation location' });
  fireEvent.click(screen.getByRole('button', { name: 'Load diagnostics' }));

  expect(await screen.findByText(/DOWNLOAD_FAILED/)).toBeInTheDocument();
  expect(invoke).toHaveBeenLastCalledWith('get_installation_diagnostics');
  fireEvent.click(screen.getByRole('button', { name: 'Copy diagnostics' }));

  expect(writeText).toHaveBeenCalledWith(JSON.stringify(diagnostics, null, 2));
  expect(await screen.findByText('Diagnostics copied.')).toBeInTheDocument();
});

it('loads and saves a custom installation folder', async () => {
  vi.mocked(invoke)
    .mockResolvedValueOnce({
      installDirectory: null,
      defaultInstallDirectory: 'C:\\Games\\Manifold',
    })
    .mockResolvedValueOnce({
      installDirectory: 'D:\\Manifold',
      defaultInstallDirectory: 'C:\\Games\\Manifold',
    });

  render(<SettingsPage />);
  const location = await screen.findByRole('textbox', {
    name: 'Installation location',
  });
  fireEvent.change(location, { target: { value: 'D:\\Manifold' } });
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));

  expect(await screen.findByText('Settings saved.')).toBeInTheDocument();
  expect(invoke).toHaveBeenLastCalledWith('set_installation_preferences', {
    installDirectory: 'D:\\Manifold',
  });
});

it('shows localized retry guidance when preferences cannot be saved', async () => {
  vi.mocked(invoke)
    .mockResolvedValueOnce({
      installDirectory: null,
      defaultInstallDirectory: 'C:\\Games\\Manifold',
    })
    .mockRejectedValueOnce(new Error('native error'));

  render(<SettingsPage />);
  await screen.findByRole('textbox', { name: 'Installation location' });
  fireEvent.click(screen.getByRole('button', { name: 'Save settings' }));

  expect(
    await screen.findByText('Settings could not be saved. Try again.'),
  ).toBeInTheDocument();
});
