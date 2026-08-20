import { invoke } from '@tauri-apps/api/core';
import { FormEvent, useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

export type AuthUser = {
  id: string;
  username: string;
  email: string;
};

type AuthPanelProps = {
  initialMode?: 'signin' | 'signup';
  onAuthenticated: (user: AuthUser) => void;
};

export function AuthPanel({
  initialMode = 'signin',
  onAuthenticated,
}: AuthPanelProps) {
  const { t } = useTranslation();
  const [mode, setMode] = useState(initialMode);
  const [step, setStep] = useState<'login' | 'code'>('login');
  const [login, setLogin] = useState('');
  const [code, setCode] = useState('');
  const [username, setUsername] = useState('');
  const [email, setEmail] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [success, setSuccess] = useState<string | null>(null);
  const [resendIn, setResendIn] = useState(0);

  useEffect(() => {
    if (resendIn <= 0) return;
    const timer = window.setInterval(
      () => setResendIn((value) => Math.max(0, value - 1)),
      1000,
    );
    return () => window.clearInterval(timer);
  }, [resendIn]);

  function changeMode(nextMode: 'signin' | 'signup') {
    setMode(nextMode);
    setStep('login');
    setCode('');
    setError(null);
    setSuccess(null);
  }

  async function requestCode(event?: FormEvent) {
    event?.preventDefault();
    setError(null);
    setSuccess(null);
    setBusy(true);
    try {
      await invoke('request_otp', { login: login.trim() });
      setStep('code');
      setResendIn(30);
    } catch {
      setError(t('auth.sendError'));
    } finally {
      setBusy(false);
    }
  }

  async function verifyCode(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setBusy(true);
    try {
      const user = await invoke<AuthUser>('verify_otp', {
        login: login.trim(),
        code: code.trim(),
      });
      onAuthenticated(user);
    } catch {
      setError(t('auth.verifyError'));
    } finally {
      setBusy(false);
    }
  }

  async function createAccount(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setSuccess(null);
    setBusy(true);
    try {
      await invoke<{ message: string }>('create_account', {
        username: username.trim(),
        email: email.trim(),
      });
      setSuccess(t('auth.accountCreated'));
      setLogin(email.trim());
    } catch {
      setError(t('auth.createError'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="auth-page" aria-labelledby="auth-title">
      <div className="auth-atmosphere" aria-hidden="true" />
      <div className="auth-panel">
        <span className="eyebrow">{t('auth.eyebrow')}</span>
        <h1 id="auth-title">
          {mode === 'signup'
            ? t('auth.createTitle')
            : step === 'code'
              ? t('auth.inboxTitle')
              : t('auth.welcome')}
        </h1>
        <p className="auth-intro">
          {mode === 'signup'
            ? t('auth.createIntro')
            : step === 'code'
              ? t('auth.codeIntro', { login })
              : t('auth.signInIntro')}
        </p>

        <div className="auth-switch" aria-label={t('auth.modeLabel')}>
          <button
            className={mode === 'signin' ? 'active' : ''}
            onClick={() => changeMode('signin')}
            type="button"
          >
            {t('auth.signIn')}
          </button>
          <button
            className={mode === 'signup' ? 'active' : ''}
            onClick={() => changeMode('signup')}
            type="button"
          >
            {t('auth.createAccount')}
          </button>
        </div>

        {mode === 'signup' ? (
          <form className="auth-form" onSubmit={createAccount}>
            <label>
              <span>{t('auth.username')}</span>
              <input
                autoComplete="username"
                maxLength={30}
                minLength={3}
                pattern="[A-Za-z0-9]{3,30}"
                required
                value={username}
                onChange={(event) => setUsername(event.target.value)}
              />
              <small>{t('auth.usernameHelp')}</small>
            </label>
            <label>
              <span>{t('auth.email')}</span>
              <input
                autoComplete="email"
                required
                type="email"
                value={email}
                onChange={(event) => setEmail(event.target.value)}
              />
            </label>
            {success && (
              <div className="form-notice success" role="status">
                <strong>{t('auth.checkInbox')}</strong>
                <span>{success}</span>
                <button type="button" onClick={() => changeMode('signin')}>
                  {t('auth.continueSignIn')}
                </button>
              </div>
            )}
            {error && (
              <p className="form-notice error" role="alert">
                {error}
              </p>
            )}
            <button className="auth-primary" disabled={busy} type="submit">
              {busy ? t('auth.creating') : t('auth.createAccount')}
            </button>
          </form>
        ) : step === 'login' ? (
          <form className="auth-form" onSubmit={requestCode}>
            <label>
              <span>{t('auth.emailOrUsername')}</span>
              <input
                autoComplete="username"
                autoFocus
                required
                value={login}
                onChange={(event) => setLogin(event.target.value)}
              />
            </label>
            {error && (
              <p className="form-notice error" role="alert">
                {error}
              </p>
            )}
            <button className="auth-primary" disabled={busy} type="submit">
              {busy ? t('auth.sending') : t('auth.sendCode')}
            </button>
            <p className="auth-note">{t('auth.noPassword')}</p>
          </form>
        ) : (
          <form className="auth-form" onSubmit={verifyCode}>
            <label>
              <span>{t('auth.code')}</span>
              <input
                aria-describedby="code-help"
                autoComplete="one-time-code"
                autoFocus
                className="otp-input"
                inputMode="numeric"
                maxLength={6}
                minLength={6}
                pattern="[0-9]{6}"
                required
                value={code}
                onChange={(event) =>
                  setCode(event.target.value.replace(/\D/g, '').slice(0, 6))
                }
              />
              <small id="code-help">{t('auth.codeHelp')}</small>
            </label>
            {error && (
              <p className="form-notice error" role="alert">
                {error}
              </p>
            )}
            <button className="auth-primary" disabled={busy} type="submit">
              {busy ? t('auth.confirming') : t('auth.confirmCode')}
            </button>
            <div className="auth-secondary-row">
              <button
                disabled={busy || resendIn > 0}
                onClick={() => requestCode()}
                type="button"
              >
                {resendIn > 0
                  ? t('auth.resendIn', { seconds: resendIn })
                  : t('auth.resend')}
              </button>
              <button
                onClick={() => {
                  setStep('login');
                  setCode('');
                  setError(null);
                }}
                type="button"
              >
                {t('auth.anotherAccount')}
              </button>
            </div>
          </form>
        )}
      </div>
    </section>
  );
}
