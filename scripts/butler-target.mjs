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
