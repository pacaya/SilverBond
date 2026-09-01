const UNICODE_WHITESPACE = /^\p{White_Space}+|\p{White_Space}+$/gu;

export function normalizeBranchEdgeLabel(raw: string): string | null {
  const trimmed = raw.replace(UNICODE_WHITESPACE, "");
  return trimmed || null;
}
