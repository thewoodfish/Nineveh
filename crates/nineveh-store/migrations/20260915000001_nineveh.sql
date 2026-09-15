-- Nineveh's own tables. Each project's state lives in a schema of its own, built
-- from its config; these tables track the builds and carry their change feeds.

CREATE SCHEMA IF NOT EXISTS nineveh;

-- One row per state schema: what it was built from, and the last version committed
-- to it. The cursor moves only in the transaction that commits the state it covers.
CREATE TABLE nineveh.projects (
    schema_name text PRIMARY KEY,
    project     text NOT NULL,
    network     text NOT NULL,
    -- Hex SHA-256 of everything that shapes derived state: the config's canonical
    -- form, the lock, the fold's semantics and the store's layout.
    fingerprint text NOT NULL,
    cursor      bigint CHECK (cursor >= 0),
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- The transactional outbox (ADR 0006): every change to a state-table row, written in
-- the transaction that commits it. Consumers tail it by (version, seq); a NOTIFY on
-- `nineveh_changes` is only a wake-up.
CREATE TABLE nineveh.changes (
    schema_name  text NOT NULL REFERENCES nineveh.projects ON DELETE CASCADE,
    version      bigint NOT NULL CHECK (version >= 0),
    -- The change's position among its version's changes.
    seq          integer NOT NULL CHECK (seq >= 0),
    table_name   text NOT NULL,
    op           text NOT NULL CHECK (op IN ('insert', 'update', 'delete')),
    key          jsonb NOT NULL,
    -- The row after the change, in the API's shape; null for a delete.
    new_row      jsonb,
    committed_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (schema_name, version, seq)
);
