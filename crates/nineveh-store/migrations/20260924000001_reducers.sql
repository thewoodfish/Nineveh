-- A project's reducers when they're written in the DSL rather than in the config's
-- `reduce:` blocks (ADR 0025). The CLI reads them from the file the config's
-- `reducers:` key names; the control plane has no filesystem, so it keeps the source
-- here. Null for a project written entirely in YAML.

ALTER TABLE nineveh.control_projects ADD COLUMN reducers text;
