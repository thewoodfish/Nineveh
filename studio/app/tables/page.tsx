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
import { Card, Empty, Ghost, Offline, Reading, filledButton } from "@/components/ui";
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
  const { tables, error, pending } = useTables(true);

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
        {pending && !tables ? (
          <Reading label="Reading this project's tables" className="space-y-3">
            {[0, 1, 2].map((i) => (
              <Card key={i} className="flex items-center gap-4 px-4 py-3">
                <Ghost className="h-4 w-40" />
                <Ghost className="h-3 w-16" />
                <Ghost className="h-3 w-24" />
              </Card>
            ))}
          </Reading>
        ) : tables && tables.length === 0 ? (
          <Empty
            icon="table_rows"
            title="No state tables yet"
            why="A state table is what your app reads: Nineveh folds the chain into it and keeps it current. Until there is one, this project is following sources and storing records nobody is asking for."
            action={
              mode === "control" ? (
                <Link href={href("/state")} className={filledButton}>
                  Build one from a source
                </Link>
              ) : undefined
            }
          />
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
