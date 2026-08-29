import assert from 'node:assert/strict';
import test from 'node:test';
import { butlerTarget } from './butler-target.mjs';

const pinnedTargets = [
  ['win32', 'x64', 'windows-x86_64'],
  ['linux', 'x64', 'linux-x86_64'],
  ['linux', 'arm64', 'linux-aarch64'],
  ['darwin', 'x64', 'macos-x86_64'],
  ['darwin', 'arm64', 'macos-aarch64'],
];

for (const [platform, architecture, expected] of pinnedTargets) {
  test(`maps ${platform}/${architecture} to ${expected}`, () => {
    assert.equal(butlerTarget(platform, architecture), expected);
  });
}

test('rejects architectures without a frozen pin', () => {
  assert.throws(
    () => butlerTarget('linux', 'ia32'),
    /not supported on Node architecture ia32/,
  );
});

test('rejects platforms without a frozen pin', () => {
  assert.throws(
    () => butlerTarget('freebsd', 'x64'),
    /not supported on Node platform freebsd/,
  );
});
