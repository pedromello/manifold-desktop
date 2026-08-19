import { invoke } from '@tauri-apps/api/core';
import { useEffect, useState } from 'react';
import { NavLink, Route, Routes } from 'react-router-dom';
import { appConfig } from './config';

type AppInfo = {
  version: string;
  environment: string;
  platform: string;
  architecture: string;
};

const pages = ['Login', 'Library', 'Downloads', 'Settings'] as const;

function Page({ name }: { name: (typeof pages)[number] }) {
  return (
    <section>
      <h1>{name}</h1>
      <p>{name} is ready for implementation.</p>
    </section>
  );
}

export function ApplicationInfo() {
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    invoke<AppInfo>('application_info', { environment: appConfig.environment })
      .then(setInfo)
      .catch(() =>
        setInfo({
          version: 'web',
          environment: appConfig.environment,
          platform: navigator.platform,
          architecture: 'browser',
        }),
      );
  }, []);
  return (
    <section>
      <h1>Application information</h1>
      {info && (
        <dl>
          <dt>Version</dt>
          <dd>{info.version}</dd>
          <dt>Environment</dt>
          <dd>{info.environment}</dd>
          <dt>Operating system</dt>
          <dd>{info.platform}</dd>
          <dt>CPU architecture</dt>
          <dd>{info.architecture}</dd>
        </dl>
      )}
    </section>
  );
}

export default function App() {
  return (
    <div className="app">
      <aside>
        <strong>Manifold</strong>
        <nav>
          {pages.map((page) => (
            <NavLink key={page} to={`/${page.toLowerCase()}`}>
              {page}
            </NavLink>
          ))}
          <NavLink to="/about">About</NavLink>
        </nav>
      </aside>
      <main>
        <Routes>
          <Route path="/" element={<Page name="Login" />} />
          {pages.map((page) => (
            <Route
              key={page}
              path={`/${page.toLowerCase()}`}
              element={<Page name={page} />}
            />
          ))}
          <Route path="/about" element={<ApplicationInfo />} />
        </Routes>
      </main>
    </div>
  );
}
