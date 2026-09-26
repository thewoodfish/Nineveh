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
