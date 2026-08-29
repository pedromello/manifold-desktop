const platforms = Object.freeze({
  win32: 'windows',
  linux: 'linux',
  darwin: 'macos',
});

const architectures = Object.freeze({
  x64: 'x86_64',
  arm64: 'aarch64',
});

export function butlerTarget(platform, architecture) {
  const mappedPlatform = platforms[platform];
  if (!mappedPlatform) {
    throw new Error(`Butler is not supported on Node platform ${platform}`);
  }
  const mappedArchitecture = architectures[architecture];
  if (!mappedArchitecture) {
    throw new Error(
      `Butler is not supported on Node architecture ${architecture}`,
    );
  }
  return `${mappedPlatform}-${mappedArchitecture}`;
}

export function butlerTargetFromTriple(targetTriple) {
  const architecture = targetTriple.split('-', 1)[0];
  const platform = targetTriple.includes('windows')
    ? 'windows'
    : targetTriple.includes('apple-darwin')
      ? 'macos'
      : targetTriple.includes('linux')
        ? 'linux'
        : null;
  if (!platform || !['x86_64', 'aarch64'].includes(architecture)) {
    throw new Error(`Butler is not pinned for target triple ${targetTriple}`);
  }
  const target = `${platform}-${architecture}`;
  if (target === 'windows-aarch64') {
    throw new Error(`Butler is not pinned for target triple ${targetTriple}`);
  }
  return target;
}
