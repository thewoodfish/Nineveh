"use client";

import { PageHeader } from "@/components/page-header";
import { Offline } from "@/components/ui";
import { Webhooks } from "@/components/webhooks";
import { useProject } from "@/lib/project";

/**
 * Nineveh calling your server, which is the opposite direction from the API console.
 *
 * Top level rather than a section of Settings: an endpoint isn't a setting, it's an
 * integration someone builds against, and it takes reading a payload shape and a few
 * tries at a URL before it works.
 */
export default function WebhooksPage() {
  const { mode, current, error } = useProject();
  if (mode === "loading") return null;
  if (mode === "offline") return <Offline error={error ?? "Nineveh isn't answering"} />;
  if (!current) {
    return (
      <div>
        <PageHeader title="Webhooks" />
        <p className="px-6 py-6 text-sm text-on-surface-variant">
          Open a project first: endpoints belong to one.
        </p>
      </div>
    );
  }
  return (
    <div>
      <PageHeader title="Webhooks" hint={current.name} />
      <div className="max-w-4xl px-6 py-6">
        <Webhooks project={current.name} />
      </div>
    </div>
  );
}
