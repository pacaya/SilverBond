/** Merge a field into a config object; pass undefined to clear it. */
export function mergeConfig<T extends object>(
  defaults: T,
  base: T | undefined,
  field: string,
  value: unknown,
): T {
  const next: Record<string, unknown> = {
    ...(defaults as Record<string, unknown>),
    ...((base ?? {}) as Record<string, unknown>),
  };
  if (value === undefined) delete next[field];
  else next[field] = value;
  return next as T;
}
