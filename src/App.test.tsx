import { render, screen } from '@testing-library/react';
import { MemoryRouter } from 'react-router-dom';
import { expect, it } from 'vitest';
import App from './App';

it('renders primary navigation', () => {
  render(
    <MemoryRouter>
      <App />
    </MemoryRouter>,
  );
  expect(screen.getByRole('link', { name: 'Downloads' })).toBeInTheDocument();
  expect(screen.getByRole('heading', { name: 'Login' })).toBeInTheDocument();
});
