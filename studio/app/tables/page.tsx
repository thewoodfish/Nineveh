"use client";

// `/tables` on its own, and the old `/tables?name=x` links, both land on a table's page.
// The list of tables is the sidebar and the overview; this route has never had anything
// of its own to show.

import { useRouter, useSearchParams } from "next/navigation";
import { Suspense, useEffect } from "react";

import { Offline } from "@/components/ui";
import { useTables } from "@/lib/hooks";
import { useProject } from "@/lib/project";

export default function TablesPage() {
  return (
    <Suspense>
      <Redirect />
    </Suspense>
  );
}

function Redirect() {
  const { name: project } = useProject();
  const wanted = useSearchParams().get("name");
  const { tables, error } = useTables();
  const router = useRouter();

  const to = wanted ?? tables?.[0]?.name;
  useEffect(() => {
    if (!to) return;
    const query = project ? `?project=${encodeURIComponent(project)}` : "";
    router.replace(`/tables/${encodeURIComponent(to)}${query}`);
  }, [to, project, router]);

  if (error && !tables) return <Offline error={error} />;
  if (tables && tables.length === 0) {
    return <p className="p-8 text-sm text-on-surface-variant">This project has no state tables.</p>;
  }
  return null;
}
