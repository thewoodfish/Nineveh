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
- A testnet key from [geomi.dev](https://geomi.dev).
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

## The machine

```sh
adduser --system --group --home /opt/nineveh nineveh
apt install postgresql caddy

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
$EDITOR /etc/nineveh/nineveh.env          # fill in the key and the OAuth pair
```

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
  Vercel with `NEXT_PUBLIC_API_URL=https://api.nineveh.dev`.

Keeping them off the VPS means the only thing that has to be single-instance is the
only thing running there.

## Deploying a new version

```sh
cd /opt/nineveh/src && git pull
cargo build --release -p nineveh-cli
install -m 755 target/release/nineveh /opt/nineveh/bin/nineveh
systemctl restart nineveh
```

`restart` stops before starting, which is what keeps you on the right side of the
one-plane rule. Migrations run at startup. A build whose fingerprint changed rebuilds
into a fresh schema and swaps when it has caught up, so the API keeps serving the old
tables throughout.

## Watching it

| | |
| --- | --- |
| `curl -s localhost:4000/health` | up, and can it reach Postgres — 503 when it can't |
| `journalctl -u nineveh -f` | what it's doing |
| `curl -s localhost:4000/control/v1/readers` | the shared streams, and `slots_free` |
| `df -h` | the one that will bite you |

Point an uptime check at `https://api.nineveh.dev/health`. It needs no auth and
answers 503 rather than 200 when Postgres is unreachable, so it's safe to act on.

`Stream-duration-limit-reached-please-reconnect` in the logs is normal: Aptos closes
stream connections at a maximum duration and expects a new one.

## Testnet only, for now

The free tier allows testnet and devnet and refuses mainnet, in code. You don't have to
configure that, and a user who asks for mainnet is told it's coming. Testnet's cap is
seven concurrent streams per account; a plane uses one shared reader plus four backfill
slots, whatever the number of projects, so five of seven with headroom left.
