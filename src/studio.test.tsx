import { cleanup, render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { afterEach, expect, it, vi } from 'vitest';
import { PublisherAdapter, PublisherStudio } from './publishing';
import { StudioPage } from './studio';

const studios: PublisherStudio[] = [
  {
    id: 'studio-1',
    slug: 'studio-one',
    name: 'Studio One',
    description: 'First studio',
    logoUrl: null,
    isPublisher: true,
    ownerId: 'user-1',
  },
  {
    id: 'studio-2',
    slug: 'studio-two',
    name: 'Studio Two',
    description: null,
    logoUrl: null,
    isPublisher: false,
    ownerId: 'user-2',
  },
];

function adapter(listGames: PublisherAdapter['listGames']): PublisherAdapter {
  return {
    listStudios: vi.fn().mockResolvedValue(studios),
    listGames,
    createDraft: vi.fn(),
    selectArchive: vi.fn(),
    inspectArchive: vi.fn(),
    publish: vi.fn(),
    cancel: vi.fn(),
  };
}

afterEach(cleanup);

it('shows a studio selector and its games without asking for technical ids', async () => {
  render(
    <MemoryRouter>
      <StudioPage
        studios={studios}
        adapter={adapter(
          vi.fn().mockResolvedValue([
            {
              id: 'game-1',
              slug: 'friendly-slug',
              title: 'A Friendly Game',
              description: 'Game description',
              status: 'PRIVATE',
              bannerUrl: null,
              iconUrl: null,
            },
          ]),
        )}
        onUnauthorized={vi.fn()}
      />
    </MemoryRouter>,
  );

  expect(screen.getByRole('combobox', { name: 'Studio' })).toBeInTheDocument();
  expect(
    await screen.findByRole('heading', { name: 'A Friendly Game' }),
  ).toBeInTheDocument();
  expect(
    screen.getByRole('button', { name: 'Manage releases' }),
  ).toBeInTheDocument();
  expect(screen.queryByText('game-1')).not.toBeInTheDocument();
});

it('maps a scoped 403 to a useful permission state', async () => {
  render(
    <MemoryRouter>
      <StudioPage
        studios={[studios[0]]}
        adapter={adapter(
          vi.fn().mockRejectedValue({
            code: 'PERMISSION_DENIED',
            message: 'Forbidden',
            retryable: false,
          }),
        )}
        onUnauthorized={vi.fn()}
      />
    </MemoryRouter>,
  );

  expect(
    await screen.findByText('Your account cannot view games for this studio.'),
  ).toBeInTheDocument();
  expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
});
