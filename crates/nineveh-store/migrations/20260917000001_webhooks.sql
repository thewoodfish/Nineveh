-- Where a project's state changes are delivered (ADR 0020): one row per named
-- endpoint, holding its signing secret, how far it has been delivered, and how it's
-- going.
--
-- Keyed by the served schema, which a rebuild's swap keeps, and deliberately without
-- a foreign key to nineveh.projects, whose row that swap replaces: an endpoint's
-- secret has to outlive the build it was configured against, or every rebuild would
-- silently break the receiver's signature checks.
CREATE TABLE nineveh.webhooks (
    schema_name       text NOT NULL,
    name              text NOT NULL,
    -- Signs every delivery. Kept as written, not hashed: signing needs the value.
    secret            text NOT NULL,
    -- The last change delivered, as a position in nineveh.changes.
    version           bigint CHECK (version >= 0),
    seq               integer CHECK (seq >= 0),
    -- Failed attempts since the last delivery, for backing off and for reporting.
    failures          integer NOT NULL DEFAULT 0,
    last_error        text,
    last_delivered_at timestamptz,
    created_at        timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (schema_name, name)
);
