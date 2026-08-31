import { spawn } from 'node:child_process';
import process from 'node:process';

const demo = process.argv[2];
if (demo && !['publisher', 'updater'].includes(demo)) {
  process.stderr.write('Demo must be either "publisher" or "updater".\n');
  process.exit(2);
}

const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm';
const npmEntry = process.env.npm_execpath;
const command = npmEntry ? process.execPath : npm;
const commandArguments = npmEntry
  ? [npmEntry, 'run', 'tauri', '--', 'dev']
  : ['run', 'tauri', '--', 'dev'];
const child = spawn(command, commandArguments, {
  env: {
    ...process.env,
    MANIFOLD_DEBUG_CONSOLE: '1',
    ...(demo ? { MANIFOLD_DEBUG_DEMO: demo } : {}),
  },
  stdio: 'inherit',
  // npm exposes its JS entrypoint to lifecycle scripts. Calling it through the
  // current Node executable avoids npm.cmd/CreateProcess incompatibilities and
  // does not require a shell on Windows.
  shell: !npmEntry && process.platform === 'win32',
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
