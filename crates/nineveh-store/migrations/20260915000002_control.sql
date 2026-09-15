-- The control plane's registry (ADR 0017): each project it manages, with the config
-- and lock it runs from. A project builds into the schema of its name.

CREATE TABLE nineveh.control_projects (
    name       text PRIMARY KEY,
    network    text NOT NULL,
    -- The project's `nineveh.yaml`, as written.
    config     text NOT NULL,
    -- Its `nineveh.lock`, pinned when the config was saved.
    lock       text NOT NULL,
    -- Whether the control plane should run its pipeline.
    running    boolean NOT NULL DEFAULT true,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
