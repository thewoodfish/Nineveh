#!/usr/bin/env bash
# Everything a fresh Ubuntu 24.04 box needs before Nineveh can be configured.
#
# Paste this into netcup's "script to run after installation" field, or run it by hand
# on a new box as root. It is safe to run twice.
#
# It deliberately stops short of starting Nineveh, because the two things it cannot
# know are secrets: your Geomi key and your GitHub OAuth pair. Those go in
# /etc/nineveh/nineveh.env afterwards, and then you start the service. Secrets do not
# belong in a provisioning script stored in a control panel.
#
# Progress goes to /var/log/nineveh-provision.log; the box will be busy for 10-20
# minutes, most of it compiling.
set -euo pipefail
exec > >(tee -a /var/log/nineveh-provision.log) 2>&1
echo "=== nineveh provision: $(date -u +%FT%TZ) ==="

REPO="${NINEVEH_REPO:-https://github.com/thewoodfish/Nineveh.git}"
export DEBIAN_FRONTEND=noninteractive

# A freshly booted box often has unattended-upgrades holding the dpkg lock.
for _ in $(seq 1 60); do
  fuser /var/lib/dpkg/lock-frontend >/dev/null 2>&1 || break
  echo "waiting for another package manager to finish..."
  sleep 5
done

echo "--- packages"
apt-get update
apt-get upgrade -y
# nano, because the minimal images ship no editor at all and the next thing anyone
# does on this box is edit a config file.
apt-get install -y --no-install-recommends \
  build-essential pkg-config git curl ca-certificates gnupg nano \
  postgresql debian-keyring debian-archive-keyring apt-transport-https

echo "--- caddy, from its own repository"
if [ ! -f /usr/share/keyrings/caddy-stable-archive-keyring.gpg ]; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    > /etc/apt/sources.list.d/caddy-stable.list
  apt-get update
fi
apt-get install -y caddy

echo "--- the nineveh user and its database"
id nineveh >/dev/null 2>&1 || adduser --system --group --home /opt/nineveh nineveh
systemctl enable --now postgresql
for _ in $(seq 1 30); do
  sudo -u postgres psql -c 'SELECT 1' >/dev/null 2>&1 && break
  sleep 2
done
sudo -u postgres psql -tAc "SELECT 1 FROM pg_roles WHERE rolname='nineveh'" | grep -q 1 \
  || sudo -u postgres createuser nineveh
sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='nineveh'" | grep -q 1 \
  || sudo -u postgres createdb --owner nineveh nineveh

echo "--- rust"
if [ ! -x /root/.cargo/bin/cargo ]; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi
CARGO=/root/.cargo/bin/cargo

echo "--- source"
if [ -d /opt/nineveh/src/.git ]; then
  git -C /opt/nineveh/src pull --ff-only
else
  git clone "$REPO" /opt/nineveh/src
fi

echo "--- build (this is the long part)"
cd /opt/nineveh/src
# Two jobs rather than one per core: rustc's parallel codegen is memory-hungry and this
# box has 8GB. Slower than -j4, and it finishes instead of being OOM-killed.
"$CARGO" build --release -p nineveh-cli -j 2

echo "--- install"
install -D -m 755 target/release/nineveh /opt/nineveh/bin/nineveh
install -D -m 755 deploy/backup.sh       /opt/nineveh/deploy/backup.sh
chown -R nineveh:nineveh /opt/nineveh

install -d -m 700 /etc/nineveh
[ -f /etc/nineveh/nineveh.env ] \
  || install -m 600 deploy/nineveh.env.example /etc/nineveh/nineveh.env

cp deploy/nineveh.service /etc/systemd/system/
cp deploy/nineveh-backup.service /etc/systemd/system/
cp deploy/nineveh-backup.timer   /etc/systemd/system/
cp deploy/Caddyfile /etc/caddy/Caddyfile
systemctl daemon-reload
systemctl enable --now nineveh-backup.timer

# Caddy answers the certificate challenge itself on port 80, so it can get one before
# there is anything behind it. Until Nineveh starts, the site is a 502 over real TLS.
systemctl reload caddy || systemctl restart caddy

cat <<'DONE'

=== provisioned ===

Nineveh is built and installed but NOT started, because it needs two secrets:

  1. nano /etc/nineveh/nineveh.env
       APTOS_API_KEY_TESTNET      a testnet key from https://geomi.dev
       NINEVEH_GITHUB_CLIENT_ID   a GitHub OAuth app, callback
       NINEVEH_GITHUB_CLIENT_SECRET   <NINEVEH_PUBLIC_URL>/auth/github/callback

     Without the GitHub pair the control plane runs with no sign-in, where every
     caller owns every project — and Caddy in front of it exposes exactly that.

  2. systemctl enable --now nineveh
     journalctl -u nineveh -f
     curl -s localhost:4000/health

The full log of this run is /var/log/nineveh-provision.log
DONE
