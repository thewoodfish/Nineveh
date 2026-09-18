"use client";

import Link from "next/link";
import { useRouter } from "next/navigation";
import { useEffect, useState } from "react";

import { setSession } from "@/lib/session";

/**
 * Where signing in ends (ADR 0018). The control plane sends the browser here with the
 * session in the URL fragment, which browsers never send to a server. It's kept, taken
 * out of the address bar and history, and Studio opens.
 */
export default function Auth() {
  const router = useRouter();
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fragment = new URLSearchParams(window.location.hash.slice(1));
    window.history.replaceState(null, "", "/auth");
    const token = fragment.get("token");
    if (token?.startsWith("nvs_")) {
      setSession(token);
      router.replace("/");
      return;
    }
    setError(fragment.get("error") ?? "Signing in didn't finish.");
  }, [router]);

  if (!error) return null;
  return (
    <div className="mx-auto mt-32 max-w-sm px-6 text-center">
      <h1 className="text-lg font-semibold tracking-tight">Couldn&apos;t sign you in</h1>
      <p className="mt-2 text-sm text-dim">{error}</p>
      <Link
        href="/"
        className="mt-6 inline-flex rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white transition-colors hover:bg-blue-500"
      >
        Try again
      </Link>
    </div>
  );
}
