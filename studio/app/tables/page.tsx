"use client";

/**
 * Every state table in the project, with what is in each one.
 *
 * The list used to live on the overview, which is a page about whether the backend is
 * keeping up. Tables are the thing people come here for, so they get the page and the
 * overview keeps the health.
 *
 * Old `/tables?name=x` links land on that table's own page instead of here.
 */

import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect } from "react";

import { PageHeader } from "@/components/page-header";
import { TableList } from "@/components/table-list";
import { Offline } from "@/components/ui";
import { useTables } from "@/lib/hooks";
import { useHref, useProject } from "@/lib/project";
import Link from "next/link";

export default function TablesPage() {
  return (
    <Suspense>
      <Tables />
    </Suspense>
  );
}

function Tables() {
  const { name: project, mode } = useProject();
  const href = useHref();
  const wanted = useSearchParams().get("name");
  const router = useRouter();
  const { tables, error } = useTables(true);

  useEffect(() => {
    if (!wanted) return;
    const query = project ? `?project=${encodeURIComponent(project)}` : "";
    router.replace(`/tables/${encodeURIComponent(wanted)}${query}`);
  }, [wanted, project, router]);
  if (wanted) return null;

  if (error && !tables) return <Offline error={error} />;

  return (
    <div>
      <PageHeader
        title="Tables"
        hint={
          <span className="text-xs">
            What this project serves. Every one is REST plus a change feed.
          </span>
        }
      />
      <div className="mx-auto max-w-6xl px-8 py-6">
        {tables && tables.length === 0 ? (
          <p className="text-sm text-on-surface-variant">
            No state tables yet.{" "}
            {mode === "control" && (
              <Link href={href("/state")} className="text-primary hover:underline">
                Build one from a source
              </Link>
            )}
            .
          </p>
        ) : (
          tables && <TableList tables={tables} />
        )}
        {mode === "control" && tables && tables.length > 0 && (
          <Link
            href={href("/state")}
            className="mt-4 inline-block text-xs font-medium text-primary hover:underline"
          >
            + New state table
          </Link>
        )}
      </div>
    </div>
  );
}
