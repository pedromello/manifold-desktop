import { Buffer } from 'node:buffer';
import { createHash } from 'node:crypto';
import process from 'node:process';
import { spawnSync } from 'node:child_process';
import {
  chmod,
  copyFile,
  mkdir,
  mkdtemp,
  readFile,
  rm,
  writeFile,
} from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { butlerTarget, butlerTargetFromTriple } from './butler-target.mjs';

const repository = join(dirname(fileURLToPath(import.meta.url)), '..');
const manifestPath = join(
  repository,
  'src-tauri',
  'vendor',
  'butler',
  'v15.30.0',
  'manifest.json',
);
const manifest = JSON.parse(await readFile(manifestPath, 'utf8'));
const target = process.env.TAURI_ENV_TARGET_TRIPLE
  ? butlerTargetFromTriple(process.env.TAURI_ENV_TARGET_TRIPLE)
  : butlerTarget(process.platform, process.arch);
const declaration = manifest.targets[target];
if (!declaration)
  throw new Error(`Butler ${manifest.version} is not pinned for ${target}`);

const digest = (bytes) => createHash('sha256').update(bytes).digest('hex');
const versionRoot = join(
  repository,
  'src-tauri',
  'resources',
  'butler',
  manifest.version,
);
const output = join(versionRoot, target);

async function outputIsValid() {
  try {
    for (const [name, expected] of Object.entries(declaration.files)) {
      if (digest(await readFile(join(output, name))) !== expected) return false;
    }
    return true;
  } catch {
    return false;
  }
}

if (await outputIsValid()) {
  process.stdout.write(
    `Butler ${manifest.version} ${target} already verified.\n`,
  );
  process.exit(0);
}

const temporary = await mkdtemp(join(tmpdir(), 'manifold-butler-'));
try {
  const archivePath = join(temporary, declaration.asset);
  const url = `https://github.com/itchio/butler/releases/download/v${manifest.version}/${declaration.asset}`;
  const response = await globalThis.fetch(url, { redirect: 'follow' });
  if (!response.ok)
    throw new Error(`Butler download failed with HTTP ${response.status}`);
  const archive = Buffer.from(await response.arrayBuffer());
  if (digest(archive) !== declaration.archiveSha256)
    throw new Error('Butler archive failed pinned SHA-256 verification');
  await writeFile(archivePath, archive);
  const extractor =
    process.platform === 'win32'
      ? spawnSync('tar', ['-xf', archivePath, '-C', temporary], {
          stdio: 'inherit',
        })
      : spawnSync('unzip', ['-q', archivePath, '-d', temporary], {
          stdio: 'inherit',
        });
  if (extractor.status !== 0)
    throw new Error('Could not extract the pinned Butler archive');
  await rm(versionRoot, { recursive: true, force: true });
  await mkdir(output, { recursive: true });
  for (const [name, expected] of Object.entries(declaration.files)) {
    const source = join(temporary, declaration.extractRoot, name);
    const bytes = await readFile(source);
    if (digest(bytes) !== expected)
      throw new Error(
        `Butler runtime file failed pinned SHA-256 verification: ${name}`,
      );
    await copyFile(source, join(output, name));
    if (name === 'butler' && process.platform !== 'win32')
      await chmod(join(output, name), 0o755);
  }
  if (!(await outputIsValid()))
    throw new Error('Prepared Butler runtime did not pass final verification');
  process.stdout.write(
    `Prepared Butler ${manifest.version} ${target} for the Tauri bundle.\n`,
  );
} finally {
  await rm(temporary, { recursive: true, force: true });
}
