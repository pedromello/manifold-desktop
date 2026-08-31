import { spawn } from 'node:child_process';
import process from 'node:process';

const demo = process.argv[2];
if (demo && !['publisher', 'updater'].includes(demo)) {
  process.stderr.write('Demo must be either "publisher" or "updater".\n');
  process.exit(2);
}

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const child = spawn(npm, ['run', 'tauri', '--', 'dev'], {
  env: {
    ...process.env,
    MANIFOLD_DEBUG_CONSOLE: '1',
    ...(demo ? { MANIFOLD_DEBUG_DEMO: demo } : {}),
  },
  stdio: 'inherit',
  // Windows cannot execute npm.cmd directly through CreateProcess on recent
  // Node releases. Arguments are fixed by this script, so cmd.exe is not fed
  // any user-controlled command text.
  shell: process.platform === 'win32',
  windowsHide: false,
});

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => child.kill(signal));
}

child.on('error', (error) => {
  process.stderr.write(`Could not start Manifold Desktop: ${error.message}\n`);
  process.exitCode = 1;
});

child.on('exit', (code, signal) => {
  process.exitCode = signal ? 1 : (code ?? 1);
});
