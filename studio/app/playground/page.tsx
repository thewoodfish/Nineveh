"use client";

import { useEffect, useState } from "react";

import { ApiConsole } from "@/components/api-console";
import { PageHeader } from "@/components/page-header";
import { useTables } from "@/lib/hooks";

/**
 * The requests an app makes against this project, ready to run and copy.
 *
 * The same catalogue a table's API tab carries, with the table still to choose — which
 * is the only thing that makes this a page of its own rather than a tab.
 */
export default function Playground() {
  const { tables } = useTables();
  const [name, setName] = useState("");

  useEffect(() => {
    if (!name && tables?.[0]) setName(tables[0].name);
  }, [tables, name]);

  const table = tables?.find((t) => t.name === name) ?? tables?.[0];

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="API" hint="Your app asking Nineveh for state" />
      <div className="flex min-h-0 flex-1 px-8 py-6">
        {tables && tables.length === 0 ? (
          <p className="text-sm text-on-surface-variant">
            This project has no state tables to query yet.
          </p>
        ) : (
          table && <ApiConsole table={table} pick={{ tables: tables ?? [], onPick: setName }} />
        )}
      </div>
    </div>
  );
}
