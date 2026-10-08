const group = new Intl.NumberFormat("en-US");
const compact = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

/** `0x1e8742…8d2a`, because a full address is 66 characters of noise. */
export const short = (a: string) => `${a.slice(0, 6)}…${a.slice(-4)}`;

/**
 * A Move integer in its asset's units, as a number.
 *
 * Wide integers arrive as strings and are divided as `BigInt`: a `u64` reaches 18
 * quintillion and a JavaScript number stops being exact at 2^53, so a lending page
 * that reaches for `Number` too early is wrong about large balances and silent about
 * it. The division happens first, in integers; only the result becomes a number.
 */
export function amount(raw: unknown, mantissa: unknown): number {
  if (raw == null) return 0;
  const value = BigInt(String(raw));
  const scale = BigInt(String(mantissa ?? 1)) || 1n;
  return Number(value / scale) + Number(value % scale) / Number(scale);
}

/** Columns of figures: grouped, two decimals, for `tabular-nums` cells. */
export const exact = (n: number) =>
  n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 });

/** Stat tiles and the hero: 18.6M rather than 18,623,120. */
export const brief = (n: number) => compact.format(n);

export const count = (n: number) => group.format(n);

/** Borrowed over supplied. Zero supply is zero used, not NaN. */
export function utilisation(cash: number, liability: number): number {
  const supplied = cash + liability;
  return supplied > 0 ? (liability / supplied) * 100 : 0;
}

/**
 * What a market's utilisation says about its liquidity.
 *
 * Utilisation is how much of the supply is lent out, so it is about whether a
 * depositor could withdraw — nothing to do with the collateral factor, which is how
 * much you may borrow *against* the asset. Tying the two together painted a market at
 * 2% utilisation critical because its collateral factor was zero, which only meant the
 * asset isn't accepted as collateral.
 *
 * The thresholds are the ordinary ones for a money market. The percentage is always
 * rendered beside the meter, so the colour never carries the meaning alone.
 */
export function severity(used: number): "good" | "warning" | "critical" {
  if (used >= 95) return "critical";
  if (used >= 80) return "warning";
  return "good";
}
