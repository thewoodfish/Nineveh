"use client";

// Drive the demo contract from the browser.
//
// The point is that a visitor needs nothing: no Aptos CLI, no Geomi key, no wallet, no
// contract of their own. Two keypairs are generated here and funded from the devnet
// faucet, which is an open API, so the only credential anyone ever handles is their own
// Nineveh key — and that is for reading what their project builds, not for this page.
//
// Two accounts rather than one because `market::buy` asserts the buyer is not the
// seller. One of them lists, the other buys.
//
// The contract's address is not baked in: `deploy/demo.sh` republishes it to devnet
// every few hours, because devnet is wiped weekly and because a free-tier project must
// start within six hours of the chain's tip. The address is read at load time.

import { useCallback, useEffect, useRef, useState } from "react";

import {
  Account,
  Aptos,
  AptosConfig,
  Ed25519PrivateKey,
  Network,
} from "@aptos-labs/ts-sdk";

/**
 * `bits.tsx`'s Button is an anchor — every other one on this site goes somewhere. These
 * do something, so they are a real button wearing the same clothes.
 */
function Act({
  onClick,
  disabled,
  tone = "quiet",
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  tone?: "primary" | "quiet";
  children: React.ReactNode;
}) {
  const tones = {
    primary:
      "bg-blue-600 text-white shadow-card hover:bg-blue-500 hover:shadow-glow focus-visible:ring-blue-400",
    quiet:
      "border border-white/15 bg-white/[0.06] text-white/80 backdrop-blur hover:border-white/25 hover:bg-white/10 hover:text-white focus-visible:ring-white/30",
  };
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className={`inline-flex cursor-pointer items-center justify-center gap-2 rounded-xl px-4 py-2.5 text-sm font-medium transition-all outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-40 ${tones[tone]}`}
    >
      {children}
    </button>
  );
}

// Where `deploy/demo.sh` publishes the current address. Overridable so the page can be
// run against a local file while developing it.
const DEMO_URL = process.env.NEXT_PUBLIC_NINEVEH_DEMO ?? "https://api.nineveh.dev/demo.json";
const STORE = "nineveh-play-keys";

type Deployment = { network: string; market: string; module: string; published_at: string };
type Entry = { id: number; what: string; who: string; state: "sending" | "done" | "failed"; detail?: string };

const ITEMS = ["lamp", "rug", "chair", "mug", "poster", "plant", "clock", "kettle"];

/**
 * Top up below a quarter of an APT. A transaction reserves its maximum gas up front, so
 * the balance that matters is the one the chain checks before running anything, not what
 * the work actually costs.
 */
const LOW_WATER = 25_000_000;

/**
 * The two accounts, kept in localStorage so a reload doesn't orphan funded keys.
 *
 * These are throwaway devnet keys with no value, which is the only reason generating
 * them in a page is reasonable at all. They are never sent anywhere.
 */
function load(): { seller: Account; buyer: Account } {
  try {
    const saved = localStorage.getItem(STORE);
    if (saved) {
      const { seller, buyer } = JSON.parse(saved) as { seller: string; buyer: string };
      return {
        seller: Account.fromPrivateKey({ privateKey: new Ed25519PrivateKey(seller) }),
        buyer: Account.fromPrivateKey({ privateKey: new Ed25519PrivateKey(buyer) }),
      };
    }
  } catch {
    // A corrupt or unreadable store is not worth recovering: make new ones.
  }
  const seller = Account.generate();
  const buyer = Account.generate();
  try {
    localStorage.setItem(
      STORE,
      JSON.stringify({
        seller: seller.privateKey.toString(),
        buyer: buyer.privateKey.toString(),
      }),
    );
  } catch {
    // Private windows: the keys just don't outlive the tab, which is harmless here.
  }
  return { seller, buyer };
}

export function Play() {
  const [demo, setDemo] = useState<Deployment | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<{ seller: Account; buyer: Account } | null>(null);
  const [held, setHeld] = useState<{ seller: number | null; buyer: number | null }>({
    seller: null,
    buyer: null,
  });
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<Entry[]>([]);
  const [copied, setCopied] = useState(false);
  /**
   * The contract this page is driving, which is not always the newest one.
   *
   * The demo contract is republished every few hours so that a new project can still
   * choose "All of its history", and that moves it to a new address. A project created
   * against the previous address keeps running perfectly and never sees another row,
   * which looks exactly like Nineveh being broken. So the address is a field: whatever
   * your project is following, paste it here and this drives that.
   */
  const [address, setAddress] = useState("");
  const [holds, setHolds] = useState<"checking" | "yes" | "no" | null>(null);
  const nextId = useRef(0);
  const listings = useRef<number[]>([]);

  // Reads go through Nineveh's own proxy rather than straight to the public fullnode.
  //
  // The anonymous allowance is per IP, so an office, a campus or a VPN exit shares one
  // between everyone behind it, and the visitor whose click fails did nothing wrong. The
  // obvious fix — a Geomi key in the page — would publish that key to anyone who opens
  // the console, because a browser cannot hold a secret. So the key stays on the server
  // and `api.nineveh.dev/aptos` forwards with it attached.
  //
  // Signing is unaffected: the proxy carries bytes a key was already applied to, and the
  // private keys never leave this tab. Unset, the page talks to the public fullnode and
  // lives on the anonymous allowance, which is fine for one person on their own address.
  const aptos = useRef(
    new Aptos(
      new AptosConfig({
        network: Network.DEVNET,
        ...(process.env.NEXT_PUBLIC_APTOS_FULLNODE
          ? { fullnode: process.env.NEXT_PUBLIC_APTOS_FULLNODE }
          : {}),
      }),
    ),
  );

  useEffect(() => {
    setAccounts(load());
    fetch(DEMO_URL)
      .then((r) => (r.ok ? r.json() : Promise.reject(new Error(`HTTP ${r.status}`))))
      .then(setDemo)
      .catch((e: unknown) => setLoadError(e instanceof Error ? e.message : String(e)));
  }, []);

  useEffect(() => {
    if (demo && !address) setAddress(demo.market);
  }, [demo, address]);

  /** Does a `market` module live there? Checked before anything is sent to it. */
  useEffect(() => {
    const at = address.trim();
    if (!/^0x[0-9a-fA-F]{1,64}$/.test(at)) {
      setHolds(at ? "no" : null);
      return;
    }
    setHolds("checking");
    let live = true;
    void aptos.current
      .getAccountModule({ accountAddress: at, moduleName: "market" })
      .then(() => live && setHolds("yes"))
      .catch(() => live && setHolds("no"));
    return () => {
      live = false;
    };
  }, [address]);

  const note = useCallback((what: string, who: string): number => {
    const id = nextId.current++;
    setLog((l) => [{ id, what, who, state: "sending" } as Entry, ...l].slice(0, 40));
    return id;
  }, []);

  const settle = useCallback((id: number, state: "done" | "failed", detail?: string) => {
    setLog((l) => l.map((e) => (e.id === id ? { ...e, state, detail } : e)));
  }, []);

  /** What each account holds, in octas, or null before it has been looked up. */
  const balances = useCallback(async () => {
    if (!accounts) return { seller: null, buyer: null };
    const read = async (a: Account) => {
      try {
        return await aptos.current.getAccountAPTAmount({ accountAddress: a.accountAddress });
      } catch {
        // An account nothing has funded yet doesn't exist, which is not an error here.
        return 0;
      }
    };
    const [seller, buyer] = await Promise.all([read(accounts.seller), read(accounts.buyer)]);
    return { seller, buyer };
  }, [accounts]);

  // Read the balances once the accounts exist, so the page can say what it has before
  // anyone presses anything.
  useEffect(() => {
    if (!accounts) return;
    let live = true;
    void balances().then((b) => {
      if (live) setHeld(b);
    });
    return () => {
      live = false;
    };
  }, [accounts, balances]);

  /**
   * Top both accounts up to something comfortable.
   *
   * Gas is reserved at the transaction's maximum, not charged at its actual cost, so an
   * account can hold enough to have paid for everything it has done and still be refused
   * for the next one. That refusal arrives as INSUFFICIENT_BALANCE_FOR_TRANSACTION_FEE,
   * which reads like an empty account and is usually a nearly-full one. Rather than make
   * anyone reason about that, every round tops up first if either account is low — the
   * faucet is free, and a devnet account is worth nothing.
   */
  const fund = useCallback(
    async (quiet = false) => {
      if (!accounts) return false;
      const have = await balances();
      const low = (v: number | null) => v === null || v < LOW_WATER;
      if (quiet && !low(have.seller) && !low(have.buyer)) return true;

      const id = note("topping the two accounts up from the devnet faucet", "faucet");
      try {
        await Promise.all(
          [accounts.seller, accounts.buyer].map((a) =>
            aptos.current.fundAccount({ accountAddress: a.accountAddress, amount: 100_000_000 }),
          ),
        );
        settle(id, "done");
        setHeld(await balances());
        return true;
      } catch (e) {
        settle(id, "failed", e instanceof Error ? e.message : String(e));
        return false;
      }
    },
    [accounts, balances, note, settle],
  );

  const fundNow = useCallback(async () => {
    setBusy(true);
    try {
      await fund();
    } finally {
      setBusy(false);
    }
  }, [fund]);

  const send = useCallback(
    async (signer: Account, who: string, fn: string, args: unknown[], label: string) => {
      if (!demo) return null;
      const id = note(label, who);
      try {
        const transaction = await aptos.current.transaction.build.simple({
          sender: signer.accountAddress,
          data: {
            function: `${address.trim()}::market::${fn}` as `${string}::${string}::${string}`,
            functionArguments: args as never,
          },
        });
        const pending = await aptos.current.signAndSubmitTransaction({ signer, transaction });
        await aptos.current.waitForTransaction({ transactionHash: pending.hash });
        settle(id, "done", pending.hash);
        return pending.hash;
      } catch (e) {
        settle(id, "failed", e instanceof Error ? e.message : String(e));
        return null;
      }
    },
    [address, demo, note, settle],
  );

  /** Credits, a listing, and a sale: one of each record the project follows. */
  const round = useCallback(async () => {
    if (!accounts || !demo) return;
    setBusy(true);
    try {
      if (!(await fund(true))) return;
      await send(accounts.buyer, "buyer", "claim_credits", [1_000], "claiming credits");

      const item = ITEMS[Math.floor(Math.random() * ITEMS.length)]!;
      const price = 50 + Math.floor(Math.random() * 400);
      const before = await aptos.current
        .view({ payload: { function: `${address.trim()}::market::next_id` as `${string}::${string}::${string}` } })
        .then((r) => Number(r[0]))
        .catch(() => null);

      const listed = await send(
        accounts.seller,
        "seller",
        "list",
        [item, price],
        `listing a ${item} for ${price}`,
      );
      if (listed && before !== null) listings.current.push(before);

      const id = listings.current.shift();
      if (id !== undefined) {
        await send(accounts.buyer, "buyer", "buy", [id], `buying listing #${id}`);
      }
    } finally {
      setBusy(false);
    }
  }, [accounts, address, demo, fund, send]);

  const cancel = useCallback(async () => {
    if (!accounts || !demo) return;
    setBusy(true);
    try {
      if (!(await fund(true))) return;
      const before = await aptos.current
        .view({ payload: { function: `${address.trim()}::market::next_id` as `${string}::${string}::${string}` } })
        .then((r) => Number(r[0]))
        .catch(() => null);
      const item = ITEMS[Math.floor(Math.random() * ITEMS.length)]!;
      const listed = await send(
        accounts.seller,
        "seller",
        "list",
        [item, 99],
        `listing a ${item} to cancel`,
      );
      if (listed && before !== null) {
        await send(accounts.seller, "seller", "cancel", [before], `cancelling listing #${before}`);
      }
    } finally {
      setBusy(false);
    }
  }, [accounts, address, demo, fund, send]);

  const copy = () => {
    if (!demo) return;
    void navigator.clipboard.writeText(address.trim()).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    });
  };

  const age = demo ? Math.round((Date.now() - Date.parse(demo.published_at)) / 60_000) : 0;

  return (
    <div className="mx-auto max-w-4xl px-6 pt-28 pb-24">
      <h1 className="font-display text-4xl font-semibold tracking-[-0.015em] text-balance text-white">
        Drive a contract. Watch it become a backend.
      </h1>
      <p className="mt-5 max-w-[62ch] text-lg leading-relaxed text-pretty text-white/55">
        This page sends real transactions to a marketplace contract on Aptos devnet. Point a
        Nineveh project at the same address and every click below turns into rows you can query
        a second later. Nothing to install, and no key but your own project&apos;s.
      </p>

      {loadError && (
        <div className="mt-10 rounded-xl border border-amber-400/25 bg-amber-400/5 p-5 text-sm text-amber-200/90">
          Couldn&apos;t read where the demo contract is ({loadError}). It is republished every few
          hours; if this persists the job may be down.
        </div>
      )}

      {demo && (
        <>
          <section className="mt-12 rounded-xl border border-white/10 bg-white/[0.03] p-6">
            <h2 className="text-sm font-semibold text-white">1. Follow this contract</h2>
            <p className="mt-2 text-sm leading-relaxed text-white/50">
              Paste it into Studio: <strong className="text-white/80">New project</strong> →{" "}
              <strong className="text-white/80">devnet</strong> → <strong className="text-white/80">Inspect</strong>{" "}
              → tick everything → <strong className="text-white/80">All of its history</strong>.
            </p>
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <input
                value={address}
                onChange={(e) => setAddress(e.target.value)}
                spellCheck={false}
                autoComplete="off"
                aria-label="The market contract to follow and drive"
                aria-invalid={holds === "no"}
                className={`min-w-0 flex-1 rounded-lg border bg-black/40 px-3 py-2 font-mono text-xs text-blue-200 outline-none transition-colors focus-visible:ring-2 focus-visible:ring-blue-400 ${
                  holds === "no" ? "border-rose-400/40" : "border-white/10"
                }`}
              />
              <Act onClick={copy}>{copied ? "Copied" : "Copy"}</Act>
            </div>

            <p className="mt-3 text-xs leading-relaxed text-white/35">
              {holds === "checking" && "Looking for a market contract there…"}
              {holds === "no" && (
                <span className="text-rose-300/80">
                  No <code>market</code> module at that address on devnet. Check it, or clear the
                  field to go back to the current one.
                </span>
              )}
              {holds === "yes" && address.trim() !== demo.market && (
                <span className="text-amber-200/80">
                  Driving your address rather than the current demo one. That is the point of this
                  field — carry on.
                </span>
              )}
              {holds === "yes" && address.trim() === demo.market && (
                <>
                  Published {age < 1 ? "just now" : `${age} minute${age === 1 ? "" : "s"} ago`}, and
                  republished every few hours: devnet is wiped weekly, and a free project has to
                  start within six hours of the chain&apos;s tip.{" "}
                  <strong className="font-normal text-white/55">
                    Each republish is a new address, and a project following the old one goes quiet
                    without saying so.
                  </strong>{" "}
                  If that has happened to you, paste your project&apos;s address above and this will
                  drive that instead — no need to build it again.
                </>
              )}
            </p>
          </section>

          <section className="mt-6 rounded-xl border border-white/10 bg-white/[0.03] p-6">
            <h2 className="text-sm font-semibold text-white">2. Make something happen</h2>
            <p className="mt-2 text-sm leading-relaxed text-white/50">
              Two throwaway accounts live in this browser — one sells, one buys, because the
              contract won&apos;t let you buy your own listing. They top themselves up from the
              devnet faucet whenever they run low, so just send things.
            </p>
            {accounts && (
              <dl className="mt-4 grid gap-2 font-mono text-xs sm:grid-cols-2">
                {(["seller", "buyer"] as const).map((who) => (
                  <div key={who} className="flex items-baseline gap-2">
                    <dt className="text-white/35">{who}</dt>
                    <dd className="min-w-0 flex-1 truncate text-white/55">
                      {accounts[who].accountAddress.toString().slice(0, 10)}…
                      <span className="ml-2 text-white/35">
                        {held[who] === null ? "" : `${(held[who] / 100_000_000).toFixed(2)} APT`}
                      </span>
                    </dd>
                  </div>
                ))}
              </dl>
            )}
            <div className="mt-5 flex flex-wrap gap-3">
              <Act onClick={() => void fundNow()} disabled={busy}>
                Top the accounts up
              </Act>
              <Act onClick={() => void round()} disabled={busy || holds !== "yes"} tone="primary">
                List and sell something
              </Act>
              <Act onClick={() => void cancel()} disabled={busy || holds !== "yes"}>
                List and cancel
              </Act>
            </div>
          </section>

          {log.length > 0 && (
            <section className="mt-6 rounded-xl border border-white/10 bg-white/[0.03] p-6">
              <h2 className="text-sm font-semibold text-white">What you sent</h2>
              <ul className="mt-4 flex flex-col gap-2 font-mono text-xs">
                {log.map((e) => (
                  <li key={e.id} className="flex items-baseline gap-3">
                    <span
                      className={
                        e.state === "done"
                          ? "text-emerald-300/80"
                          : e.state === "failed"
                            ? "text-rose-300/80"
                            : "text-white/35"
                      }
                    >
                      {e.state === "done" ? "ok" : e.state === "failed" ? "no" : "··"}
                    </span>
                    <span className="text-white/35">{e.who}</span>
                    <span className="min-w-0 flex-1 text-white/65">
                      {e.what}
                      {e.state === "failed" && e.detail && (
                        <span className="block text-rose-300/60">{e.detail}</span>
                      )}
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}
