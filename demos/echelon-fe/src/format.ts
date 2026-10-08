const group = new Intl.NumberFormat("en-US");

/** `0x1e8742…8d2a`, because a full address is 66 characters of noise. */
export const short = (a: string) => `${a.slice(0, 6)}…${a.slice(-4)}`;

/**
 * A Move integer in its asset's units.
 *
 * Wide integers arrive as strings and are parsed as `BigInt`: a `u64` goes to 18
 * quintillion and a JavaScript number stops being exact at 2^53, so a lending page
 * that reaches for `Number` is wrong about large balances and silent about it.
 */
export function units(raw: unknown, mantissa: unknown): string {
  if (raw == null) return "—";
  const value = BigInt(String(raw));
  const scale = BigInt(String(mantissa ?? 1)) || 1n;
  const whole = value / scale;
  const cents = ((value % scale) * 100n) / scale;
  return `${group.format(whole)}.${String(cents).padStart(2, "0")}`;
}

export const count = (n: number) => group.format(n);

/** Borrowed over supplied, as a percentage. Zero supply is zero used, not NaN. */
export function utilisation(cash: unknown, liability: unknown): number {
  const c = BigInt(String(cash ?? 0));
  const l = BigInt(String(liability ?? 0));
  const supplied = c + l;
  return supplied > 0n ? Number((l * 1000n) / supplied) / 10 : 0;
}
