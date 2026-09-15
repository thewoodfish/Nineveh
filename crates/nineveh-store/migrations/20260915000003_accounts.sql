-- Accounts, sessions and project API keys (ADR 0018). Tokens are stored only as the
-- SHA-256 of what's presented, so the database never holds a working token.

-- One row per person, keyed by their GitHub user id: logins can change, ids can't.
CREATE TABLE nineveh.accounts (
    id          bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    github_id   bigint NOT NULL UNIQUE,
    login       text NOT NULL,
    name        text,
    avatar_url  text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    updated_at  timestamptz NOT NULL DEFAULT now()
);

-- Studio's sessions.
CREATE TABLE nineveh.sessions (
    token_hash  bytea PRIMARY KEY,
    account_id  bigint NOT NULL REFERENCES nineveh.accounts ON DELETE CASCADE,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL
);
CREATE INDEX sessions_account ON nineveh.sessions (account_id);

-- Sign-ins in flight: the `state` sent to GitHub, spent on its return.
CREATE TABLE nineveh.oauth_states (
    state       text PRIMARY KEY,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- Who owns each project; null for projects made in local mode.
ALTER TABLE nineveh.control_projects
    ADD COLUMN owner_id bigint REFERENCES nineveh.accounts;
CREATE INDEX control_projects_owner ON nineveh.control_projects (owner_id);

-- Keys an app uses to reach one project's API.
CREATE TABLE nineveh.api_keys (
    id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    project      text NOT NULL REFERENCES nineveh.control_projects (name) ON DELETE CASCADE,
    label        text NOT NULL,
    -- The key's first characters, to recognize it by.
    prefix       text NOT NULL,
    key_hash     bytea NOT NULL UNIQUE,
    created_at   timestamptz NOT NULL DEFAULT now(),
    last_used_at timestamptz,
    revoked_at   timestamptz
);
CREATE INDEX api_keys_project ON nineveh.api_keys (project);
