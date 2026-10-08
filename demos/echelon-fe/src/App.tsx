import { configured, type Row } from "./api";
import { count, short, units, utilisation } from "./format";
import { useLive } from "./useLive";

const CONFIG = `# nineveh.yaml — the whole of it
name: echelon
network: mainnet
sources:
  vault:   { resource: "0xc6bc…21ba::lending::Vault" }
  market:  { resource: "0xc6bc…21ba::lending::Market" }
state:
  vaults:  { mirror: vault }
  markets: { mirror: market }`;

/** A SimpleMap arrives as { data: [{ key, value }] }. */
type Entry = { key?: { inner?: string } };
const entries = (map: unknown): Entry[] =>
  (map as { data?: Entry[] } | null)?.data ?? [];

function Stat({ n, label }: { n: string; label: string }) {
  return (
    <div className="stat">
      <div className="n">{n}</div>
      <div className="l">{label}</div>
    </div>
  );
}

function Markets({ markets }: { markets: Row[] }) {
  if (markets.length === 0)
    return <p className="empty">No markets yet — they appear as the chain writes them.</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>Asset</th>
          <th className="num">Supplied</th>
          <th className="num">Borrowed</th>
          <th className="num">Cash</th>
          <th className="num">Util</th>
          <th />
          <th className="num">Collateral factor</th>
          <th />
        </tr>
      </thead>
      <tbody>
        {markets.map((m) => {
          const used = utilisation(m.total_cash, m.total_liability);
          const supplied = BigInt(String(m.total_cash)) + BigInt(String(m.total_liability));
          return (
            <tr key={String(m.address)}>
              <td className="key mono">{String(m.asset_name ?? "—")}</td>
              <td className="num mono">{units(supplied, m.asset_mantissa)}</td>
              <td className="num mono">{units(m.total_liability, m.asset_mantissa)}</td>
              <td className="num mono">{units(m.total_cash, m.asset_mantissa)}</td>
              <td className="num mono">{used.toFixed(1)}%</td>
              <td>
                <div className="bar">
                  <i style={{ width: `${Math.min(used, 100)}%` }} />
                </div>
              </td>
              <td className="num mono">{(Number(m.collateral_factor_bps) / 100).toFixed(0)}%</td>
              <td className="mono faint">{m.paused ? "paused" : "live"}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Vaults({ vaults, markets }: { vaults: Row[]; markets: Row[] }) {
  const named = new Map(markets.map((m) => [String(m.address), String(m.asset_name)]));
  // A market this project hasn't mirrored yet has no name to show, which is honest:
  // the address is what the chain said.
  const label = (e: Entry) => named.get(e.key?.inner ?? "") ?? short(e.key?.inner ?? "?");
  if (vaults.length === 0)
    return (
      <p className="empty">
        No vaults yet — one appears the moment an account touches the protocol.
      </p>
    );
  return (
    <table>
      <thead>
        <tr>
          <th>Account</th>
          <th>Collateral in</th>
          <th>Borrowed from</th>
          <th className="num">#</th>
          <th className="num">#</th>
          <th className="num">Last changed at version</th>
        </tr>
      </thead>
      <tbody>
        {vaults.map((v) => {
          const collateral = entries(v.collaterals);
          const debt = entries(v.liabilities);
          return (
            <tr key={String(v.address)}>
              <td className="key mono">{short(String(v.address))}</td>
              <td className="mono">{collateral.map(label).join(", ") || "—"}</td>
              <td className="mono">{debt.map(label).join(", ") || "—"}</td>
              <td className="num mono">{collateral.length}</td>
              <td className="num mono">{debt.length}</td>
              <td className="num mono faint">{String(v._version)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export default function App() {
  const { markets, vaults, error, connected, feed } = useLive();

  if (!configured)
    return (
      <div className="wrap">
        <header>
          <h1>No key.</h1>
          <p className="lede">
            Set <span className="mono">VITE_ECHELON_KEY</span> — see{" "}
            <span className="mono">.env.example</span>.
          </p>
        </header>
      </div>
    );

  const supplied = markets.reduce(
    (a, m) => a + Number(BigInt(String(m.total_cash)) / BigInt(String(m.asset_mantissa || 1))),
    0,
  );
  const borrowed = markets.reduce(
    (a, m) =>
      a + Number(BigInt(String(m.total_liability)) / BigInt(String(m.asset_mantissa || 1))),
    0,
  );

  return (
    <div className="wrap">
      <header>
        <h1>Echelon&apos;s lending state, live.</h1>
        <p className="lede">
          Every account&apos;s collateral and debt, and every market&apos;s totals, read
          from the Aptos chain as it commits. <b>None of this came from an event.</b>{" "}
          Echelon&apos;s own liquidator guide says to index{" "}
          <span className="mono">SupplyEvent</span> to discover vault addresses and then
          read each account&apos;s resource yourself — this follows the resource, so the
          vault is the row.
        </p>
        <p className="lede">
          Built from the config at the bottom of this page. There is no backend.
        </p>
        <p>
          <span className="status">
            <span className={`dot${connected ? "" : " off"}`} />
            {error ?? (connected ? "following the chain" : "connecting…")}
          </span>
        </p>
      </header>

      <div className="grid">
        <Stat n={count(markets.length)} label="markets followed" />
        <Stat n={count(vaults.length)} label="accounts with a vault" />
        <Stat n={count(Math.round(supplied + borrowed))} label="supplied, all assets" />
        <Stat n={count(Math.round(borrowed))} label="borrowed, all assets" />
      </div>

      <h2>Markets</h2>
      <div className="card">
        <Markets markets={markets} />
      </div>
      <p className="note">
        Utilisation is borrowed ÷ supplied, computed here from the mirrored totals. The
        interest index moves on its own clock, which is why a borrower&apos;s debt
        changes with no event behind it.
      </p>

      <h2>Vaults</h2>
      <div className="card">
        <Vaults vaults={vaults} markets={markets} />
      </div>
      <p className="note">
        One row per account, appearing as accounts transact. This is the table a
        liquidator bot assembles by polling every address it has ever seen.
      </p>

      <h2>Changes, as they commit</h2>
      <div className="card">
        <div className="feed">
          {feed.length === 0 && <div className="faint">waiting for the chain to move…</div>}
          {feed.map((c) => (
            <div key={`${c.version}.${c.seq}`}>
              <b>{c.op}</b> {c.table} <span className="faint">@{c.version}</span>
            </div>
          ))}
        </div>
      </div>

      <h2>The whole backend</h2>
      <pre>{CONFIG}</pre>
    </div>
  );
}
