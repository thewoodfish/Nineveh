"use client";

import { useSearchParams } from "next/navigation";
import { Suspense } from "react";

import { DataGrid } from "@/components/data-grid";
import { Offline } from "@/components/ui";
import { useTables } from "@/lib/hooks";

export default function TablesPage() {
  return (
    <Suspense>
      <TableView />
    </Suspense>
  );
}

function TableView() {
  const name = useSearchParams().get("name");
  const { tables, error } = useTables();
  if (error && !tables) return <Offline error={error} />;
  if (!tables) return null;
  const table = tables.find((t) => t.name === name) ?? tables[0];
  if (!table) {
    return <p className="p-8 text-sm text-on-surface-variant">This project has no state tables.</p>;
  }
  // Keyed by name so switching tables starts fresh.
  return <DataGrid key={table.name} table={table} />;
}
