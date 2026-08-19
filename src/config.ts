export const environments = {
  development: { productionLike: false },
  staging: { productionLike: true },
  production: { productionLike: true },
} as const;

export type AppEnvironment = keyof typeof environments;

export function loadConfig(
  value: string = import.meta.env.VITE_APP_ENV ?? 'development',
) {
  if (!(value in environments))
    throw new Error(`Invalid VITE_APP_ENV: ${value}`);
  const environment = value as AppEnvironment;
  return { environment };
}

export const appConfig = loadConfig();
