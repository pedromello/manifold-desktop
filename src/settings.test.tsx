import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { SettingsPage } from './settings';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));

afterEach(() => {
  cleanup();
  vi.mocked(invoke).mockReset();
  vi.mocked(open).mockReset();
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
