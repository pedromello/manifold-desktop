import { invoke } from '@tauri-apps/api/core';
import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AuthPanel } from './auth';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

afterEach(cleanup);

it('completes the email OTP sign-in flow', async () => {
  const onAuthenticated = vi.fn();
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === 'request_otp') return Promise.resolve({ message: 'sent' });
    if (command === 'verify_otp') {
      return Promise.resolve({
        id: 'user-1',
        username: 'pedro',
        email: 'pedro@example.com',
      });
    }
    return Promise.resolve({});
  });

  render(<AuthPanel onAuthenticated={onAuthenticated} />);
  fireEvent.change(screen.getByLabelText('Email or username'), {
    target: { value: 'pedro@example.com' },
  });
  fireEvent.click(screen.getByRole('button', { name: 'Send login code' }));

  const code = await screen.findByRole('textbox', { name: /6-digit code/ });
  fireEvent.change(code, { target: { value: '123456' } });
  fireEvent.click(screen.getByRole('button', { name: 'Confirm code' }));

  await vi.waitFor(() =>
    expect(onAuthenticated).toHaveBeenCalledWith({
      id: 'user-1',
      username: 'pedro',
      email: 'pedro@example.com',
    }),
  );
  expect(invoke).toHaveBeenCalledWith('request_otp', {
    login: 'pedro@example.com',
  });
  expect(invoke).toHaveBeenCalledWith('verify_otp', {
    login: 'pedro@example.com',
    code: '123456',
  });
});
