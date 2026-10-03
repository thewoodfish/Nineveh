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
  const [funded, setFunded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [log, setLog] = useState<Entry[]>([]);
  const [copied, setCopied] = useState(false);
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

  const note = useCallback((what: string, who: string): number => {
    const id = nextId.current++;
    setLog((l) => [{ id, what, who, state: "sending" } as Entry, ...l].slice(0, 40));
    return id;
  }, []);

  const settle = useCallback((id: number, state: "done" | "failed", detail?: string) => {
    setLog((l) => l.map((e) => (e.id === id ? { ...e, state, detail } : e)));
  }, []);

  /** Fund both accounts. Devnet's faucet is an API, which is why this page can exist. */
  const fund = useCallback(async () => {
    if (!accounts) return;
    setBusy(true);
    const id = note("funding two accounts from the devnet faucet", "faucet");
    try {
      await Promise.all(
        [accounts.seller, accounts.buyer].map((a) =>
          aptos.current.fundAccount({ accountAddress: a.accountAddress, amount: 100_000_000 }),
        ),
      );
      setFunded(true);
      settle(id, "done");
    } catch (e) {
      settle(id, "failed", e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  }, [accounts, note, settle]);

  const send = useCallback(
    async (signer: Account, who: string, fn: string, args: unknown[], label: string) => {
      if (!demo) return null;
      const id = note(label, who);
      try {
        const transaction = await aptos.current.transaction.build.simple({
          sender: signer.accountAddress,
          data: {
            function: `${demo.module}::${fn}` as `${string}::${string}::${string}`,
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
    [demo, note, settle],
  );

  /** Credits, a listing, and a sale: one of each record the project follows. */
  const round = useCallback(async () => {
    if (!accounts || !demo) return;
    setBusy(true);
    try {
      await send(accounts.buyer, "buyer", "claim_credits", [1_000], "claiming credits");

      const item = ITEMS[Math.floor(Math.random() * ITEMS.length)]!;
      const price = 50 + Math.floor(Math.random() * 400);
      const before = await aptos.current
        .view({ payload: { function: `${demo.module}::next_id` as `${string}::${string}::${string}` } })
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
  }, [accounts, demo, send]);

  const cancel = useCallback(async () => {
    if (!accounts || !demo) return;
    setBusy(true);
    try {
      const before = await aptos.current
        .view({ payload: { function: `${demo.module}::next_id` as `${string}::${string}::${string}` } })
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
  }, [accounts, demo, send]);

  const copy = () => {
    if (!demo) return;
    void navigator.clipboard.writeText(demo.market).then(() => {
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
              <code className="min-w-0 flex-1 overflow-x-auto rounded-lg bg-black/40 px-3 py-2 font-mono text-xs text-blue-200">
                {demo.market}
              </code>
              <Act onClick={copy}>{copied ? "Copied" : "Copy"}</Act>
            </div>
            <p className="mt-3 text-xs text-white/35">
              Published {age < 1 ? "just now" : `${age} minute${age === 1 ? "" : "s"} ago`}. It is
              republished every few hours, because devnet is wiped weekly and a free project has to
              start within six hours of the chain&apos;s tip.
            </p>
          </section>

          <section className="mt-6 rounded-xl border border-white/10 bg-white/[0.03] p-6">
            <h2 className="text-sm font-semibold text-white">2. Make something happen</h2>
            <p className="mt-2 text-sm leading-relaxed text-white/50">
              Two throwaway accounts live in this browser — one sells, one buys, because the
              contract won&apos;t let you buy your own listing. Fund them once from the devnet
              faucet, then send whatever you like.
            </p>
            <div className="mt-5 flex flex-wrap gap-3">
              <Act onClick={() => void fund()} disabled={busy || funded}>
                {funded ? "Funded" : "Fund the two accounts"}
              </Act>
              <Act onClick={() => void round()} disabled={busy || !funded} tone="primary">
                List and sell something
              </Act>
              <Act onClick={() => void cancel()} disabled={busy || !funded}>
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
