# Deploying Nineveh

One VPS, one Postgres, one process. This is the whole thing.

It is a VPS rather than a platform-as-a-service for one reason: **only one control
plane may run against a database.** Two conflict at commit time and halt each other's
projects, deliberately and unretryably. Render, Railway and Fly all default to rolling
deploys — new instance up, then old one down — which would hit that on every deploy.
`systemctl restart` stops before it starts, which is the shape this needs.

## Before you start

- A VPS. Testnet-only, a handful of projects: 3 vCPU / 4 GB / 80 GB is plenty. Watch
  disk rather than CPU — a busy project can write a gigabyte an hour.
- DNS for `api.nineveh.dev` pointing at it.
- A Geomi key per network you mean to serve, from [geomi.dev](https://geomi.dev). Keys
  are issued per network and Studio offers only the networks you have one for, so a
  plane serving both testnet and devnet needs both.
- A GitHub OAuth app. **Not optional** — see the warning below.

## Sign-in is not optional

Without `NINEVEH_GITHUB_CLIENT_ID`, the plane runs in **local mode**: no sign-in, and
every caller is the owner of every project.

It refuses to *listen* on anything but loopback in that mode — but that check is on the
listen address, not on who can reach it. Caddy in front of `127.0.0.1:4000` satisfies
the check and exposes the whole thing to the internet. Set the GitHub variables. There
is no second guard.

Create the OAuth app at <https://github.com/settings/developers> with:

| | |
| --- | --- |
| Homepage URL | `https://studio.nineveh.dev` |
| Authorization callback URL | `https://api.nineveh.dev/auth/github/callback` |

The callback must be exactly `<NINEVEH_PUBLIC_URL>/auth/github/callback`.

## The quick way

If your provider takes a script to run after installation — netcup does — paste
[`provision.sh`](provision.sh) into it. It does everything on this page up to the point
where secrets are needed: packages, Caddy's repository, the user, the database, Rust,
the build, the systemd units and the Caddyfile. Then you fill in
`/etc/nineveh/nineveh.env` and start the service.

It is safe to run twice, logs to `/var/log/nineveh-provision.log`, and deliberately
holds no secrets — a provisioning script lives in a control panel, which is not where a
Geomi key or an OAuth secret belongs.

The rest of this page is the same thing by hand.

## The machine

Caddy isn't in Ubuntu's default repositories — or the version there is old — so it
comes from its own:

```sh
apt update
apt install -y build-essential pkg-config git curl postgresql \
               debian-keyring debian-archive-keyring apt-transport-https

curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  | tee /etc/apt/sources.list.d/caddy-stable.list
apt update && apt install -y caddy
```

`build-essential` is for the C compiler the TLS crate needs; there is no OpenSSL to
install, because Nineveh uses rustls.

Then the user and the database:

```sh
adduser --system --group --home /opt/nineveh nineveh

sudo -u postgres createuser nineveh
sudo -u postgres createdb --owner nineveh nineveh
```

## Build and install

Build on the server, or build locally for the same target and copy the binary up.

```sh
git clone https://github.com/thewoodfish/Nineveh.git /opt/nineveh/src
cd /opt/nineveh/src
cargo build --release -p nineveh-cli

install -D -m 755 target/release/nineveh /opt/nineveh/bin/nineveh
install -D -m 755 deploy/backup.sh       /opt/nineveh/deploy/backup.sh
chown -R nineveh:nineveh /opt/nineveh
```

## Configure

```sh
install -d -m 700 /etc/nineveh
install -m 600 /opt/nineveh/src/deploy/nineveh.env.example /etc/nineveh/nineveh.env
```

Then fill in the keys and the OAuth pair. A fresh server has no editor you can count on
— `nano: command not found` is the usual greeting, and `$EDITOR` is unset, which makes
the line the path on its own, so the shell tries to execute the file and reports
`Permission denied` as though the file were the problem. So write it rather than edit
it:

```sh
cat >> /etc/nineveh/nineveh.env <<'EOF'
APTOS_API_KEY_TESTNET=aptoslabs_...
NINEVEH_DATABASE_URL=postgres:///nineveh
EOF

grep . /etc/nineveh/nineveh.env           # read it back before restarting
```

Appending keeps the file's `600`. `sed -i '/^NAME=/d'` takes a line out again.

## Start it

```sh
cp deploy/nineveh.service /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now nineveh

journalctl -u nineveh -f
curl -s localhost:4000/health             # {"status":"ok","database":true,…}
```

Then TLS:

```sh
cp deploy/Caddyfile /etc/caddy/Caddyfile
systemctl reload caddy

curl -s https://api.nineveh.dev/health
```

Caddy gets a certificate on the first request and renews it itself.

## The demo contract (removed)

There used to be a `nineveh.dev/play` page that fired a market contract on devnet from
the browser, with a timer on this box republishing that contract and a Caddy proxy
forwarding the page's fullnode reads with Nineveh's own Geomi key. All of it is gone: the
[tutorial](../docs/first-backend.md) now has the reader publish `examples/03-market`
themselves and drive it from the Aptos CLI, which is what an Aptos developer already has.

The proxy is the part worth understanding before putting anything like it back. It was
unauthenticated by necessity — a page cannot hold a secret — so anyone who found
`/aptos/*` could spend the key's monthly credit, and in the end something did: it
returned `429 Blocked due to MonthlyCredit cap` for every request, which the page
reported as "no contract at that address".

On a box that ran it, this takes it out:

```sh
systemctl disable --now nineveh-demo.timer
rm -f /etc/systemd/system/nineveh-demo.service /etc/systemd/system/nineveh-demo.timer
rm -f /etc/systemd/system/caddy.service.d/demo.conf
rm -f /etc/nineveh/demo.env               # then revoke that key at https://geomi.dev
rm -rf /var/lib/nineveh/public            # demo.json lived here

cp deploy/Caddyfile /etc/caddy/Caddyfile  # no /demo.json, no /aptos/*
systemctl daemon-reload && systemctl restart caddy

curl -s -o /dev/null -w '%{http_code}\n' https://api.nineveh.dev/demo.json   # 404
```

Revoke the key rather than just unmounting it: it was reachable by anyone for as long as
the proxy was up. The keys in `nineveh.env` that the processors use are separate and
unaffected.

## Backups

```sh
cp deploy/nineveh-backup.{service,timer} /etc/systemd/system/
systemctl daemon-reload
systemctl enable --now nineveh-backup.timer

systemctl start nineveh-backup            # prove it works now, not in a crisis
ls -lh /var/backups/nineveh/
```

Daily, seven kept. **Copy them off the machine** — a backup on the disk you're
protecting against is not a backup. Restore is
`pg_restore -d nineveh --clean --if-exists <file>` with the plane stopped.

## The frontends

`nineveh.dev` and `studio.nineveh.dev` don't belong on this box.

- **`site/`** is a static export. `npm run build` produces `site/out`, which is plain
  HTML — point Cloudflare Pages or Vercel at it and forget about it.
- **`studio/`** is a Next app that only talks to the API over HTTP. Deploy it to
  Vercel with

  ```
  NEXT_PUBLIC_NINEVEH_API=https://api.nineveh.dev
  ```

  That name matters: Studio reads `NEXT_PUBLIC_NINEVEH_API` and falls back to
  `http://127.0.0.1:4000` when it isn't set, so a Studio built without it looks fine
  and can't reach anything. It's a `NEXT_PUBLIC_` variable, which means it is baked in
  at build time — setting it in Vercel is not enough on its own, you have to redeploy
  after. To check a deployed Studio, search its JavaScript for `127.0.0.1:4000`; if
  it's there, it was built without the variable.

Keeping them off the VPS means the only thing that has to be single-instance is the
only thing running there.

## Deploying a new version

```sh
git config --global --add safe.directory /opt/nineveh/src   # once, see below
cd /opt/nineveh/src
git pull --ff-only
/root/.cargo/bin/cargo build --release -p nineveh-cli -j 2
install -m 755 target/release/nineveh /opt/nineveh/bin/nineveh
chown -R nineveh:nineveh /opt/nineveh
systemctl restart nineveh
```

Three things that look like mistakes and aren't:

- **`cargo` by absolute path.** `provision.sh` installs rustup with `--no-modify-path`,
  so `cargo` is at `/root/.cargo/bin/cargo` and not on any PATH. `cargo: command not
  found` here means Rust is installed and unexported, not missing.
- **`safe.directory`.** The install chowns the tree to `nineveh`, and git refuses to run
  in a repository owned by another user: `detected dubious ownership`. Adding the
  exception is the fix, once per machine.
- **`-j 2`.** rustc's parallel codegen is memory-hungry. On a 4–8GB box the default job
  count gets the build OOM-killed partway through; two jobs is slower and finishes.

The `chown` after `install` matters as much as the install: the unit runs as `nineveh`,
and a binary left owned by root after a build as root can't be executed by it.

`restart` stops before starting, which is what keeps you on the right side of the
one-plane rule. Migrations run at startup. A build whose fingerprint changed rebuilds
into a fresh schema and swaps when it has caught up, so the API keeps serving the old
tables throughout.

## Letting one account onto mainnet

The free tier follows testnet and devnet. To let a single account start a mainnet
project — for a demo, not as a tier change — name its GitHub login:

```sh
cat >> /etc/nineveh/nineveh.env <<'EOF'
APTOS_API_KEY_MAINNET=aptoslabs_...
NINEVEH_MAINNET_ACCOUNTS=yourlogin
EOF
```

```sh
systemctl restart nineveh
journalctl -u nineveh | grep "by exception"   # it says so, every time it applies
```

To take it away:

```sh
sed -i '/^NINEVEH_MAINNET_ACCOUNTS=/d' /etc/nineveh/nineveh.env
systemctl restart nineveh
```

Nothing in the source changed, so there is nothing to revert and nothing to forget.
Leaving `APTOS_API_KEY_MAINNET` behind is harmless: without a name in the allowlist the
tier refuses mainnet to everybody again.

It widens **one account**, deliberately. Widening the tier instead would let every
account that has ever signed up start a mainnet project on the organization's stream
credit, and running that credit out stops every project at once, on every network.

A mainnet project also streams the full firehose if it follows any resource or table
(ADR 0004), so expect it in the hourly `stream draw` line.

## Watching it

| | |
| --- | --- |
| `curl -s localhost:4000/health` | up, and can it reach Postgres — 503 when it can't |
| `journalctl -u nineveh -f` | what it's doing |
| `journalctl -u nineveh \| grep 'stream draw'` | hourly, per network: GiB and dollars a month at the current rate |
| `sudo -u postgres psql -d nineveh -c 'select name, network, running from nineveh.control_projects'` | what the plane is meant to be running |
| `sudo -u postgres psql -d nineveh -c 'select schema_name, cursor from nineveh.projects'` | how far each build has got |

`sudo -u postgres` rather than `$NINEVEH_DATABASE_URL`: the variable is in the unit's
environment, not your shell, so `psql "$NINEVEH_DATABASE_URL"` expands to nothing and
falls back to a socket connection as `root` — which has no role, and says so in a way
that looks like the database is broken.

`/control/v1/readers` reports the same streams in more detail, but every `/control/v1`
route needs a signed-in session, so plain `curl` from the box answers `sign in to use
Nineveh` rather than anything useful. On a hosted plane the journal and the database are
the operator's way in; `curl` works only in local mode, where there is no sign-in.
| `df -h` | the one that will bite you |

Point an uptime check at `https://api.nineveh.dev/health`. It needs no auth and
answers 503 rather than 200 when Postgres is unreachable, so it's safe to act on.

`Stream-duration-limit-reached-please-reconnect` in the logs is normal: Aptos closes
stream connections at a maximum duration and expects a new one.

## Which networks a project can use

Two independent gates, and Studio shows which one said no.

- **The tier**, in code: the free tier allows testnet and devnet and refuses mainnet.
  Nothing to configure, and a user who asks for mainnet is told it's coming.
- **Your keys**, from the environment file: a network with no key can't be streamed
  whatever the tier says, so Studio greys it out rather than accepting a project it
  would fail to start.

`journalctl -u nineveh` reports both at startup — `ready to stream` names the networks
it holds a key for, and a warning names any the tier allows that you haven't keyed.

No mainnet means no mainnet stream caps to think about. Testnet's cap is seven
concurrent streams per account; a plane uses one shared reader plus four backfill slots,
whatever the number of projects, so five of seven with headroom left.
