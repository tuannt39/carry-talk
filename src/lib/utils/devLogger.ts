const isDev = import.meta.env.DEV;

export function devWarn(message?: unknown, ...optionalParams: unknown[]): void {
  if (!isDev) {
    return;
  }

  console.warn(message, ...optionalParams);
}

export function devError(message?: unknown, ...optionalParams: unknown[]): void {
  if (!isDev) {
    return;
  }

  console.error(message, ...optionalParams);
}
