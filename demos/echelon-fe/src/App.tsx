import { configured, type Row } from "./api";
import { amount, count, exact, severity, short, utilisation } from "./format";
import { useFlash } from "./useFlash";
import { useLive } from "./useLive";

const CONFIG = `name: echelon
network: mainnet

sources:
  vault:   { resource: "0xc6bc…21ba::lending::Vault" }
  market:  { resource: "0xc6bc…21ba::lending::Market" }

state:
  vaults:  { mirror: vault }
  markets: { mirror: market }`;

/** A SimpleMap arrives as { data: [{ key, value }] }. */
type Entry = { key?: { inner?: string } };
const entries = (map: unknown): Entry[] => (map as { data?: Entry[] } | null)?.data ?? [];

function Meter({ used }: { used: number }) {
  const state = severity(used);
  return (
    <div className={`meter ${state}`} title={`${used.toFixed(1)}% of the supply is lent out`}>
      <i style={{ width: `${Math.min(Math.max(used, 1.5), 100)}%` }} />
    </div>
  );
}

function Markets({ markets, flashing }: { markets: Row[]; flashing: Set<string> }) {
  if (markets.length === 0)
    return <p className="empty">No markets yet — a row appears as the chain writes one.</p>;
  return (
    <table>
      <thead>
        <tr>
          <th>Asset</th>
          <th className="num">Supplied</th>
          <th className="num">Borrowed</th>
          <th className="num">Available</th>
          <th className="num">Utilisation</th>
          <th className="meter-col" />
          <th className="num">Max LTV</th>
        </tr>
      </thead>
      <tbody>
        {markets.map((m) => {
          const cash = amount(m.total_cash, m.asset_mantissa);
          const debt = amount(m.total_liability, m.asset_mantissa);
          const used = utilisation(cash, debt);
          const id = String(m.address);
          return (
            <tr key={id} className={flashing.has(id) ? "flash" : undefined}>
              <td className="key" title={id}>
                <span className="asset">{String(m.asset_name ?? "—")}</span>
                {Boolean(m.paused) && <span className="chip">paused</span>}
              </td>
              <td className="num">{exact(cash + debt)}</td>
              <td className="num">{exact(debt)}</td>
              <td className="num muted">{exact(cash)}</td>
              <td className={`num ${severity(used)}-text`}>
                {used.toFixed(1)}%
              </td>
              <td className="meter-col">
                <Meter used={used} />
              </td>
              <td className="num muted">
                {(Number(m.collateral_factor_bps) / 100).toFixed(0)}%
              </td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

function Vaults({
  vaults,
  markets,
  flashing,
}: {
  vaults: Row[];
  markets: Row[];
  flashing: Set<string>;
}) {
  const named = new Map(markets.map((m) => [String(m.address), String(m.asset_name)]));
  // A market this project hasn't mirrored yet has no name to show, and the address is
  // what the chain said — better than inventing one.
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
          <th className="num">Last moved at</th>
        </tr>
      </thead>
      <tbody>
        {vaults.map((v) => {
          const collateral = entries(v.collaterals);
          const debt = entries(v.liabilities);
          const id = String(v.address);
          return (
            <tr key={id} className={flashing.has(id) ? "flash" : undefined}>
              <td className="key mono">{short(id)}</td>
              <td>
                {collateral.length === 0 ? (
                  <span className="muted">—</span>
                ) : (
                  collateral.map((e, i) => (
                    <span key={i} className="chip">
                      {label(e)}
                    </span>
                  ))
                )}
              </td>
              <td>
                {debt.length === 0 ? (
                  <span className="muted">—</span>
                ) : (
                  debt.map((e, i) => (
                    <span key={i} className="chip owing">
                      {label(e)}
                    </span>
                  ))
                )}
              </td>
              <td className="num mono muted">{String(v._version)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

export default function App() {
  const { markets, vaults, error, connected, feed } = useLive();
  const marketFlash = useFlash(markets, "address");
  const vaultFlash = useFlash(vaults, "address");

  if (!configured)
    return (
      <main className="wrap">
        <h1>No key</h1>
        <p className="lede">
          Set <code>VITE_ECHELON_KEY</code> — see <code>.env.example</code>.
        </p>
      </main>
    );

  // Deliberately no cross-asset total. Summing APT, USDC and WLFI token counts would
  // need prices this page doesn't have, and a headline number that is quietly wrong is
  // worse than none — especially in front of the people who know what APT trades at.
  // Amounts stay per market, in that market's own asset, where they are true.
  const borrowing = vaults.filter((v) => entries(v.liabilities).length > 0).length;

  return (
    <main className="wrap">
      <header>
        <p className="eyebrow">
          <span className={`dot${connected ? "" : " off"}`} />
          {error ?? (connected ? "Following Aptos mainnet" : "Connecting…")}
        </p>
        <h1>
          Echelon&apos;s lending state,
          <br />
          as the chain writes it.
        </h1>
        <p className="lede">
          Every account&apos;s collateral and debt, and every market&apos;s totals.{" "}
          <b>None of it arrived as an event.</b> Echelon&apos;s own liquidator guide
          says to index <code>SupplyEvent</code> for vault addresses and then read each
          account yourself — this follows the resource, so the vault <em>is</em> the row.
        </p>

        <div className="hero">
          <div className="figure">{count(vaults.length)}</div>
          <div className="beneath">
            accounts whose position this page knows, across {count(markets.length)}{" "}
            {markets.length === 1 ? "market" : "markets"} — {count(borrowing)} of them
            borrowing. Each one appeared the moment it touched the protocol.
          </div>
        </div>
      </header>

      <section>
        <div className="tiles">
          <div className="tile">
            <div className="label">Markets followed</div>
            <div className="value">{count(markets.length)}</div>
          </div>
          <div className="tile">
            <div className="label">Events this needed</div>
            <div className="value">0</div>
          </div>
          <div className="tile">
            <div className="label">Lines of backend</div>
            <div className="value">0</div>
          </div>
        </div>
      </section>

      <section>
        <h2>Markets</h2>
        <div className="card">
          <Markets markets={markets} flashing={marketFlash} />
        </div>
        <p className="note">
          Utilisation is how much of a market&apos;s supply is lent out, so it is what
          a depositor wanting to withdraw cares about; max LTV is a different question,
          how much you may borrow against the asset. The interest index moves on its own
          clock — which is why a borrower&apos;s debt changes with no event behind it.
        </p>
      </section>

      <section>
        <h2>Vaults</h2>
        <div className="card">
          <Vaults vaults={vaults} markets={markets} flashing={vaultFlash} />
        </div>
        <p className="note">
          One row per account, appearing as accounts transact. This is the table a
          liquidator bot assembles by polling every address it has ever seen.
        </p>
      </section>

      <section>
        <h2>Changes, as they commit</h2>
        <div className="card feed">
          {feed.length === 0 ? (
            <p className="empty">Waiting for the chain to move…</p>
          ) : (
            feed.map((c) => (
              <div key={`${c.version}.${c.seq}`} className="line">
                <span className={`op ${c.op}`}>{c.op}</span>
                <span className="mono">{c.table}</span>
                <span className="muted mono">@{c.version}</span>
              </div>
            ))
          )}
        </div>
      </section>

      <section>
        <h2>The whole backend</h2>
        <pre>{CONFIG}</pre>
        <p className="note">
          No processor, no database, no server. Two resources followed, two tables
          served, a REST API and this change feed.
        </p>
      </section>

      <footer>
        Built with{" "}
        <a href="https://nineveh.dev" target="_blank" rel="noreferrer">
          Nineveh
        </a>{" "}
        — the reactive backend for Aptos. Data read live from Echelon on mainnet.
      </footer>
    </main>
  );
}
