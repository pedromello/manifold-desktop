import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { DownloadsPage } from './downloads';

const installation = vi.hoisted(() => ({
  cancel: vi.fn(),
  install: vi.fn(),
  progress: {
    capyvarias: {
      gameSlug: 'capyvarias',
      title: 'Capyvarias',
      phase: 'extracting',
      downloadedBytes: 2048,
      totalBytes: 2048,
      version: '1.0.0',
      error: null,
      bytesPerSecond: undefined as number | undefined,
      estimatedSecondsRemaining: undefined as number | undefined,
    },
  },
}));

vi.mock('./installation', () => ({
  useInstallations: () => installation,
}));

afterEach(() => {
  cleanup();
  installation.cancel.mockReset();
  installation.install.mockReset();
  installation.progress.capyvarias.phase = 'extracting';
  installation.progress.capyvarias.bytesPerSecond = undefined;
  installation.progress.capyvarias.estimatedSecondsRemaining = undefined;
});

it('shows a localized speed and ETA only while bytes are downloading', () => {
  installation.progress.capyvarias.phase = 'downloading';
  installation.progress.capyvarias.bytesPerSecond = 1_024;
  installation.progress.capyvarias.estimatedSecondsRemaining = 90;
  render(
    <MemoryRouter>
      <DownloadsPage />
    </MemoryRouter>,
  );

  expect(screen.getByText(/1 KB\/s/)).toBeInTheDocument();
  expect(screen.getByText(/2 min remaining/)).toBeInTheDocument();
});

it('announces extraction progress with accessible progress semantics', () => {
  render(
    <MemoryRouter>
      <DownloadsPage />
    </MemoryRouter>,
  );

  expect(screen.getByText('Extracting files')).toBeInTheDocument();
  expect(
    screen.getByRole('progressbar', { name: 'Capyvarias progress' }),
  ).toHaveAttribute('aria-valuenow', '100');
  expect(
    screen.getByRole('button', { name: 'Cancel download' }),
  ).toBeInTheDocument();
});

it('shows localized feedback when cancellation fails', async () => {
  installation.cancel.mockRejectedValueOnce(new Error('native error'));
  render(
    <MemoryRouter>
      <DownloadsPage />
    </MemoryRouter>,
  );

  fireEvent.click(screen.getByRole('button', { name: 'Cancel download' }));

  expect(
    await screen.findByText('This download could not be cancelled.'),
  ).toBeInTheDocument();
});
