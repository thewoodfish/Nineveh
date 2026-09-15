# 0018. Sign in with GitHub; reach projects with API keys

- Status: Accepted
- Date: 2026-09-15

## Context

Nineveh will be hosted. Developers sign in to Studio on the web, create projects, and
call each project's API from their apps. They run nothing themselves. The control plane
(ADR 0017) has no authentication: it listens on localhost, and anyone who can reach it
can read, change or delete any project. Hosting needs two things it doesn't have:

- **Who is using Studio**, so each account sees and changes only its own projects.
- **Which app is calling a project's API**, so the owner can hand out access, see it
  used, and revoke it.

Developers mostly have GitHub accounts, and GitHub sign-in means Nineveh stores no
passwords. A project's state is derived from public chain data, so an API key
identifies and meters callers rather than guarding a secret. Keys will sit in browser
code, as Supabase's and Firebase's do.

## Decision

**Accounts come from GitHub.** Studio sends the browser to `GET /auth/github`. The
control plane redirects to GitHub's authorize page with a single-use `state`, stored
for ten minutes. GitHub redirects back to `GET /auth/github/callback`. The control plane
then:

1. checks and spends the `state`;
2. exchanges the code for a GitHub token;
3. reads the user's id, login, name and avatar;
4. discards the GitHub token.

An account is keyed by its GitHub user id, which never changes; the login can. No
GitHub scope is requested: the public profile is enough.

**Sessions are Nineveh's own bearer tokens.** After sign-in the control plane creates a
session. It redirects to the configured Studio URL with the token in the URL fragment
(`#token=`), which browsers don't send to servers. That target URL is fixed, so the
redirect can't be pointed elsewhere. Studio keeps the token and sends it as
`Authorization: Bearer`. Sessions last 30 days, and signing out deletes one.
Bearer tokens, unlike cookies, need no CSRF defence, and they work while Studio and the
API are on different origins.

**Project API keys** are created in Studio, shown once, and revocable. An app sends one
as `Authorization: Bearer`, an `apikey` header, or an `apikey` query parameter. The
query parameter exists because a browser's `EventSource` can't set headers. A key
reaches exactly one project's state API and change feed, never the control API.

**Tokens.** Every token is 32 random bytes from the OS, hex-encoded behind a prefix:
`nvs_` for sessions, `nvk_` for keys. The database stores only its SHA-256, so a
leaked database doesn't leak working tokens. A token is found by the hash of what was
presented, and there's no secret to compare. SHA-256 is enough here because the
tokens are random, not chosen by people; password hashing would add nothing.

**Ownership.** A project belongs to the account that created it (`owner_id`). The
control API lists and changes only the caller's projects. For any other project it
answers 404, as if the project didn't exist. Project names stay global (ADR 0017),
because each names a schema and an API path. A taken name is refused as it is today.

**Two modes.** `nineveh up` is in *hosted* mode when it has GitHub credentials
(`NINEVEH_GITHUB_CLIENT_ID` and `NINEVEH_GITHUB_CLIENT_SECRET`) and a Studio URL:

- every control API request needs a session;
- every project API request needs the project's key, or its owner's session.

Without GitHub credentials it's in *local* mode:

- no sign-in, and every project is reachable, as before;
- it refuses to listen on anything but a loopback address.

**Studio reads the change feed with `fetch`** rather than `EventSource`, so it can send
its session in a header, and no token ends up in a URL, a log or history.

## Alternatives considered

- **Email and password.** We'd store password hashes and own reset flows, which need an
  email service. GitHub is where developers already are. Email sign-in can be added
  later as a second identity on the same account.
- **A hosted auth provider** (Clerk, Auth0, Supabase Auth). It would take the least auth
  code on our side, but it adds a vendor, a monthly cost, and a dependency on them for
  every sign-in.
- **Cookie sessions.** Cookies need CSRF defences, and they only work when Studio and the
  API share a site. Browsers increasingly block them otherwise.
- **Signed stateless tokens (JWT).** Revoking one needs a denylist anyway, and a
  database lookup per request is cheap next to the queries it guards.
- **Secret keys with write access.** The state API is read-only: reducers are the only
  writers (ADR 0005). There's nothing for a secret key to guard yet.

## Consequences

- Hosting is safe to try. No one reaches a project they don't own without its key.
- Running hosted needs a GitHub OAuth app, and its callback URL must match the control
  plane's public address.
- Developers put keys in browser code, knowing a key can be revoked but not hidden.
  Rate limits and usage per key come next, on the same table.
- Local mode keeps the no-login setup for us and self-hosters, and it can't be exposed
  by accident.
