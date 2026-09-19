// Formatting chain values for people. Wide integers stay exact: they're strings
// parsed with BigInt, never Number.

export function formatInteger(value: string | number | bigint | null | undefined): string {
  if (value === null || value === undefined || value === "") return "—";
  try {
    return BigInt(value).toLocaleString("en-US");
  } catch {
    return String(value);
  }
}

/** `0x1234…abcd`, for addresses and hex. */
export function shortHex(value: string, head = 6, tail = 4): string {
  if (!value.startsWith("0x") || value.length <= 2 + head + tail + 1) return value;
  return `${value.slice(0, 2 + head)}…${value.slice(-tail)}`;
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "—";
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m ${seconds % 60}s`;
  const hours = Math.floor(minutes / 60);
  if (hours < 48) return `${hours}h ${minutes % 60}m`;
  const days = Math.floor(hours / 24);
  if (days < 730) return `${days}d`;
  return `${Math.floor(days / 365)}y`;
}

/** How far a backfill has come, from 0 to 1. */
export function progress(start: string | null, cursor: string | null, target: string | null): number | null {
  if (!start || !target) return null;
  try {
    const s = BigInt(start);
    const t = BigInt(target);
    const c = cursor ? BigInt(cursor) : s;
    if (t <= s) return 1;
    const done = Number(((c - s) * 10000n) / (t - s)) / 10000;
    return Math.min(1, Math.max(0, done));
  } catch {
    return null;
  }
}

export function behind(cursor: string | null, target: string | null): string | null {
  if (!cursor || !target) return null;
  try {
    const gap = BigInt(target) - BigInt(cursor);
    return gap > 0n ? gap.toString() : "0";
  } catch {
    return null;
  }
}

/** Bytes as a person reads them: `1.4 GB`, `512 MB`, `— ` for nothing known. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let n = bytes;
  let unit = 0;
  while (n >= 1024 && unit < units.length - 1) {
    n /= 1024;
    unit += 1;
  }
  const decimals = unit === 0 || n >= 100 || Number.isInteger(n) ? 0 : n >= 10 ? 1 : 2;
  return `${n.toFixed(decimals)} ${units[unit]}`;
}
