import { useEffect, useRef, useState } from "react";
import type { Row } from "./api";

/**
 * The keys whose `_version` just moved, for a moment.
 *
 * A dashboard that claims to be live should look it. `_version` is on every row and
 * says when it last changed, so a row that has moved since the previous read is the
 * row to light up — no diffing of values, and nothing to get wrong when a number
 * happens to repeat.
 */
export function useFlash(rows: Row[], key: string): Set<string> {
  const seen = useRef(new Map<string, string>());
  const [flashing, setFlashing] = useState<Set<string>>(new Set());

  useEffect(() => {
    const moved = new Set<string>();
    for (const row of rows) {
      const id = String(row[key]);
      const version = String(row._version);
      const before = seen.current.get(id);
      if (before !== undefined && before !== version) moved.add(id);
      seen.current.set(id, version);
    }
    if (moved.size === 0) return;
    setFlashing(moved);
    const done = setTimeout(() => setFlashing(new Set()), 1100);
    return () => clearTimeout(done);
  }, [rows, key]);

  return flashing;
}
