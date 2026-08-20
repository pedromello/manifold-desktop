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
