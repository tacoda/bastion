# Bastion server on DigitalOcean

<!-- voice-register: informational -->
<!-- voice-english: ste -->

This plan puts `bastion-server` on an Ubuntu Droplet, behind TLS, so the
desktop app on a Mac can add it as a remote server. The hard part is the
sandbox: every agent turn runs in a micro-VM, and on Linux that VM needs
`/dev/kvm`. DigitalOcean does not document nested virtualization, so step 3
tests it before you install anything. If the test fails, stop and use another
host. The plan is written for an employer's DigitalOcean team, so it keeps
work resources, billing, and keys apart from personal ones.

Nothing here was run end to end. Each fact names its source: a file in this
repository, or a URL.

## 1. The verdict: can DigitalOcean run it?

**Probably yes, on a Droplet with dedicated vCPUs, but DigitalOcean does not
support it officially. You must test it on the real Droplet.**

What the sources say:

- DigitalOcean's product documentation does not mention nested
  virtualization. The Droplet docs list plans, limits, and features, and
  none of them names `/dev/kvm` or nested virtualization
  (https://docs.digitalocean.com/products/droplets/details/features/,
  https://docs.digitalocean.com/products/droplets/details/limits/).
- A DigitalOcean staff answer from 2021 says: "Currently, DigitalOcean
  Droplets support KVM or nested virtualization in all regions. We do not
  recommend nested virtualization on the platform because there is often
  very poor performance."
  (https://www.digitalocean.com/community/questions/does-digitalocean-support-kvm-or-nested-virtulzation)
- A 2024 answer repeats the same text
  (https://www.digitalocean.com/community/questions/question-about-our-work).
- A 2016 staff answer says nested virtualization "may be possible but it is
  not a planned or supported feature"
  (https://www.digitalocean.com/community/questions/does-digitalocean-support-nested-kvm-now).
- Bare Metal GPUs are single-tenant servers with 8 GPUs, sold for AI and ML
  work. Their feature page lists Ubuntu 22.04 or 24.04 with SSH keys, and
  says nothing about virtualization
  (https://docs.digitalocean.com/products/bare-metal-gpus/details/features/).
  They are far larger than Bastion needs. Their price was not checked.
- DigitalOcean MicroVMs are Firecracker VMs in public preview, built for
  agent sandboxes (https://digitalocean.com/blog/microvms-public-preview).
  Bastion cannot use them today. The sandbox is the `iii-sandbox` worker in
  `worker-compose.yaml`, and it boots its VMs on the same host as the
  engine. No setting in this repository sends a sandbox to another host.

What the code needs:

- Doctor refuses to start the stack on Linux without `/dev/kvm`
  (`crates/bastion-server/src/doctor.rs`, the `sandbox` check).
- iii's sandbox adapter needs glibc Linux on x86_64 or aarch64. The user who
  runs compose needs read and write access to `/dev/kvm`, and `libcap-ng0`
  must be installed (https://iii.dev/docs/upgrading/sandbox-worker.md,
  section "Supported hosts").

The consequence: `/dev/kvm` can exist and the VMs can still be slow, because
they run nested. Measure a real job in step 5 before you trust the Droplet
with work.

If step 3 fails, these are the options that remain:

- Run the server on the Mac itself. Apple Silicon runs the sandbox natively
  (`doctor.rs`), and the local server needs no token or TLS.
- Use a provider that documents nested virtualization. Google Compute Engine
  does (https://cloud.google.com/compute/docs/instances/nested-virtualization/overview).
  This plan does not cover it.
- Use a dedicated server with hardware virtualization, where `/dev/kvm` is
  native. This plan does not name one, because none was researched.

## 2. How the pieces fit

Read this before the steps. Each fact comes from the code.

| Part | Address | Open to the internet |
|---|---|---|
| `bastion-server` HTTP API and event stream | `127.0.0.1:49190` | No. Caddy proxies to it. |
| iii engine, main listener | `127.0.0.1:49160` | Never. The engine trusts the worker name a connection declares (`worker-compose.yaml`). |
| iii engine, governed listener | `127.0.0.1:49161` | Never. |
| Caddy, TLS | `0.0.0.0:443` | Yes. |
| SSH | `0.0.0.0:22` | Yes, from your IP only if you can. |

The server takes three flags for TLS: `--listen IP`, `--tls-cert PEM`, and
`--tls-key PEM` (`crates/bastion-server/src/main.rs`,
`crates/bastion-server/src/tls.rs`). It refuses a non-loopback `--listen`
without a certificate and key. It reads the certificate once, at start
(`tls::acceptor` runs once in `serve`). It does not reload a renewed
certificate.

A restart of the server also stops the stack, because the stack goes down
with the server (DECISIONS.md D13, `main.rs`). A certificate renewal that
restarts the server stops every running job. So this plan does not give the
server its own certificate. It keeps the server on loopback and puts Caddy in
front. Caddy gets and renews the Let's Encrypt certificate with no restart of
Bastion.

This is safe because the server asks for a token on every request, also on
loopback. Only the owner's token in `data/token`, or a token issued to a
person, gets through (`crates/bastion-server/src/access.rs`,
`crates/bastion-server/src/http.rs`). Caddy adds no access, and removes none.

Caddy flushes a `text/event-stream` response at once, so the server's event
stream (`GET /events`) works through it with no extra setting
(https://caddyserver.com/docs/caddyfile/directives/reverse_proxy, section
"Streaming").

## 3. The steps

Run the `doctl` commands on the Mac. Run the other commands on the Droplet,
as the user each step names.

### Step 1: team, project, SSH key, and Droplet

Use the employer's DigitalOcean team, not a personal one. On DigitalOcean,
a team holds infrastructure and billing. "Each team has separate billing and
its own payment information unless it belongs to an organization"
(https://docs.digitalocean.com/platform/teams/). Do these things first:

1. Ask the team owner whether you may run this Droplet on the work team,
   and who pays for it. The Droplet will hold work code, a GitHub token, and
   model provider keys.
2. Ask the owner for a role that can create Droplets, firewalls, domains,
   and projects. The roles are on https://docs.digitalocean.com/platform/teams/roles/.
3. Keep personal Droplets on your personal team. Do not move them to the work
   team, and do not put work resources on the personal team.

Projects group resources inside a team
(https://docs.digitalocean.com/products/projects/). Put the Droplet, its
firewall, and its domain in one project, so the employer sees the whole
cost in one place.

Install `doctl` and sign in to the work team with its own context:

```sh
brew install doctl
doctl auth init --context work
doctl auth switch --context work
doctl account get
```

Make sure `doctl account get` shows your work email and team. If it shows
the personal team, stop and switch the context.

Create the project, and import an SSH key that you use only for work:

```sh
ssh-keygen -t ed25519 -f ~/.ssh/bastion_do -C "bastion on DigitalOcean"
doctl projects create --name bastion --purpose "Bastion server" --environment Production
doctl projects list
doctl compute ssh-key import bastion-do --public-key-file ~/.ssh/bastion_do.pub
doctl compute ssh-key list
```

Choose the Droplet. Bastion's stack starts up to 8 sandbox VMs at once, each
with 1 vCPU and 1024 MB by default (`worker-compose.yaml`, `iii-sandbox`).
The engine and about 25 workers run beside them. Nested VMs share vCPUs
badly, so choose dedicated vCPUs. Prices are the Regular CPU column on
https://www.digitalocean.com/pricing/droplets:

| Plan | Slug | vCPUs | Memory | Disk | Price |
|---|---|---|---|---|---|
| General Purpose (recommended) | `g-4vcpu-16gb` | 4 dedicated | 16 GiB | 50 GiB | $126.00/month, $0.18750/hour |
| CPU-Optimized (smaller) | `c-4` | 4 dedicated | 8 GiB | 50 GiB | $84.00/month, $0.12500/hour |

The 8 GiB plan cannot hold 8 sandboxes at 1024 MB and the stack together.
If you choose it, lower the job limit on each workspace. The Premium Intel
prices were not read; check them on the pricing page.

Choose the region nearest to you. List the regions and sizes first:

```sh
doctl compute region list
doctl compute size list | grep -E 'g-4vcpu-16gb|c-4 '
```

Create the Droplet in the project, with a tag for the firewall:

```sh
doctl compute droplet create bastion \
  --region nyc3 \
  --size g-4vcpu-16gb \
  --image ubuntu-24-04-x64 \
  --ssh-keys "$(doctl compute ssh-key list --format ID,Name --no-header | awk '$2=="bastion-do"{print $1}')" \
  --project-id <project-id> \
  --tag-name bastion \
  --enable-monitoring \
  --wait
doctl compute droplet list --format ID,Name,PublicIPv4
```

Replace `nyc3` with your region, and `<project-id>` with the ID from
`doctl projects list`.

### Step 2: Ubuntu 24.04 and first hardening

Use Ubuntu 24.04. The release binaries are built on `ubuntu-24.04` runners
(`.github/workflows/release.yml`), so an older Ubuntu can lack the glibc
they need.

Sign in as root once, and create the user that runs Bastion:

```sh
ssh -i ~/.ssh/bastion_do root@<droplet-ip>
```

On the Droplet, as root:

```sh
adduser --disabled-password --gecos "" bastion
usermod -aG sudo,kvm bastion
install -d -m 700 -o bastion -g bastion /home/bastion/.ssh
install -m 600 -o bastion -g bastion /root/.ssh/authorized_keys /home/bastion/.ssh/authorized_keys
passwd bastion
```

The `bastion` user needs a password for `sudo` only. SSH stays key-only.

Turn off password and root sign-in for SSH:

```sh
cat > /etc/ssh/sshd_config.d/10-bastion.conf <<'EOF'
PasswordAuthentication no
KbdInteractiveAuthentication no
PermitRootLogin no
EOF
sshd -t && systemctl restart ssh
```

Before you close the root session, open a second terminal on the Mac and
make sure this works:

```sh
ssh -i ~/.ssh/bastion_do bastion@<droplet-ip>
```

Turn on security updates:

```sh
apt update && apt -y upgrade
apt install -y unattended-upgrades
dpkg-reconfigure -plow unattended-upgrades
```

Ubuntu's unattended upgrades do not reboot by default. A kernel update needs
a reboot that you plan. After a reboot, the service comes back by itself
(step 5).

### Step 3: check `/dev/kvm` before anything else

Do this step before you install anything more. If it fails, destroy the
Droplet. Billing is hourly, so a failed test costs less than one hour.

As `bastion`, on the Droplet:

```sh
ls -l /dev/kvm
grep -c -E 'vmx|svm' /proc/cpuinfo
sudo apt install -y cpu-checker
kvm-ok
id
```

The test passes when all of these are true:

- `/dev/kvm` exists, with group `kvm`.
- The `grep` count is more than 0.
- `kvm-ok` prints "KVM acceleration can be used".
- `id` lists the group `kvm`. If it does not, sign out and sign in again.

If `/dev/kvm` is missing, try to load the module once:

```sh
sudo modprobe kvm_intel || sudo modprobe kvm_amd
ls -l /dev/kvm
```

If it is still missing, stop. Destroy the Droplet and read the options in
section 1:

```sh
doctl compute droplet delete bastion
```

Doctor only checks that `/dev/kvm` exists. A VM that really boots is the
real test, which is the test job at the end of step 5.

### Step 4: install `iii` 0.25.0 and the host tools

The stack needs these on the host:

| Tool | Why | Source |
|---|---|---|
| `iii` 0.25.0 exactly | Runs the stack. Doctor fails any other version. | `doctor.rs`, `III_VERSION` |
| `libcap-ng0` | The sandbox adapter needs it on Linux. | https://iii.dev/docs/upgrading/sandbox-worker.md |
| `git` | The factory runs `git` on the host, and the server reads `git config user.name` for the owner's name. | `main.rs`, `git_user` |
| `gh`, signed in | Pushes and pull requests from the host. Optional: landing locally works without it. | `doctor.rs`, `github`; README.md |
| `minisign` | Lets the installer check the release signature, not only the checksums. | `scripts/install.sh` |
| `curl`, `tar` | The installers. | `scripts/install.sh` |

As `bastion`:

```sh
sudo apt install -y libcap-ng0 git gh minisign curl tar
```

Install `iii` 0.25.0 with no prompt. The installer takes the version as its
last argument, and puts `iii` in `~/.local/bin` by default
(https://docs.iii.dev/docs/install.md, sections "Install a pre-release",
"Install without a prompt", and "Control the install with environment
variables"):

```sh
curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- --non-interactive 0.25.0
~/.local/bin/iii --version
```

The last command must print `0.25.0`.

Do not run `iii update`. It moves `iii` past 0.25.0, and Doctor then refuses
to start the stack.

Set the owner's name. The server uses `git config user.name` when it has no
`--owner` flag (`main.rs`), and the service has no `--owner` flag:

```sh
git config --global user.name "Ian Johnson"
```

### Step 5: install `bastion-server` as a systemd user service

Sign in over SSH as `bastion` directly. Do not use `sudo su bastion`:
`systemctl --user` needs the user's own session.

Install the server and the CLI, with the service (README.md, and
`scripts/install.sh`):

```sh
curl -fsSL https://tacoda.github.io/bastion/install.sh | sh -s -- --service
```

The installer writes `~/.config/systemd/user/bastion-server.service` with
this command line (`scripts/install.sh`):

```
<prefix>/lib/bastion-<version>/bin/bastion-server
  --checkout <prefix>/lib/bastion-<version>/share/bastion
  --data ~/.local/share/dev.tacoda.bastion/server
  --port 49190 --engine ws://127.0.0.1:49160
```

That unit listens on loopback, which is what this plan wants. Do not add
`--listen` or the TLS flags.

Give the service a `PATH` that holds `iii`. The unit sets no `PATH`, and
Doctor needs `iii` "on the PATH Bastion starts with" (`doctor.rs`). Check
first:

```sh
systemctl --user show-environment | grep '^PATH='
```

If `~/.local/bin` is not in it, add a drop-in. The installer rewrites the
unit file, but not the drop-in folder:

```sh
mkdir -p ~/.config/systemd/user/bastion-server.service.d
cat > ~/.config/systemd/user/bastion-server.service.d/path.conf <<'EOF'
[Service]
Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin
EOF
systemctl --user daemon-reload
systemctl --user restart bastion-server
```

Keep the service running after you sign out:

```sh
sudo loginctl enable-linger bastion
```

Check that it runs, and read its first lines:

```sh
systemctl --user status bastion-server
journalctl --user -u bastion-server -n 50
```

Look for `bastion-server: serving http on 127.0.0.1:49190`. Look also for
`stack: ` with a state, not `stack did not start` (`main.rs`). The stack's
own log is `~/.local/share/dev.tacoda.bastion/server/compose.log`.

Check that only the expected ports listen beyond loopback:

```sh
sudo ss -tlnp
```

Only `22` (and `443` after step 6) may listen on `0.0.0.0` or `[::]`. Every
iii and Bastion port must be on `127.0.0.1`. If a worker listens on a public
address, step 7's firewalls still block it. Write the port down and report
it, because this plan does not know every worker's port.

The VM test comes after step 8, when the app can reach the server: add one
small workspace and run one short job. Time its first turn. If the VM does
not boot, or a turn is far slower than on the Mac, this Droplet is not
usable.

### Step 6: DNS name and TLS

Pick a name, such as `bastion.example.com`, under a domain the employer
controls. DigitalOcean DNS works when the domain's name servers point to
DigitalOcean. Ask who owns the domain before you change it.

On the Mac:

```sh
doctl compute domain records create example.com \
  --record-type A --record-name bastion \
  --record-data <droplet-ip> --record-ttl 300
dig +short bastion.example.com
```

Install Caddy from its apt repository. These commands come from
https://caddyserver.com/docs/install, section "Debian, Ubuntu, Raspbian":

```sh
sudo apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg
sudo chmod o+r /etc/apt/sources.list.d/caddy-stable.list
sudo apt update
sudo apt install caddy
```

The package runs Caddy as the system service `caddy`. Write its
configuration:

```sh
sudo tee /etc/caddy/Caddyfile <<'EOF'
bastion.example.com {
	reverse_proxy 127.0.0.1:49190
}
EOF
sudo systemctl reload caddy
journalctl -u caddy -n 50
```

Caddy gets a certificate from Let's Encrypt and renews it by itself
(https://caddyserver.com/docs/automatic-https). It has two challenges. The
TLS-ALPN challenge needs only port 443, and it is on by default. The HTTP
challenge needs port 80 (same page, section "ACME challenges"). This plan
keeps port 80 closed, so Caddy uses TLS-ALPN. Step 7 must open 443 before
Caddy can get the first certificate.

Renewal needs no action. Caddy renews in the background, and Bastion does
not restart.

Check from the Mac. A request with no token must get `401`:

```sh
curl -sS -o /dev/null -w '%{http_code}\n' https://bastion.example.com/me
```

### Step 7: firewalls

Open only SSH and HTTPS. Use two layers: the Cloud Firewall at
DigitalOcean's edge, and `ufw` on the Droplet. Cloud Firewalls cost
nothing, and block all traffic that no rule allows
(https://docs.digitalocean.com/products/networking/firewalls/).

The ports that must stay closed to the internet are `49190` (the server),
`49160` and `49161` (the iii engine). They bind to `127.0.0.1` today. The
firewalls are the second guard, in case a change binds one of them wider.
The engine trusts the worker name a connection declares
(`worker-compose.yaml`), so an open engine port gives a stranger the
factory.

On the Mac, create the Cloud Firewall for the `bastion` tag. Replace
`<your-ip>` with your public IP. If your IP changes often, use
`0.0.0.0/0,address:::/0` for port 22, and rely on key-only SSH:

```sh
doctl compute firewall create --name bastion --tag-names bastion \
  --inbound-rules "protocol:tcp,ports:22,address:<your-ip>/32 protocol:tcp,ports:443,address:0.0.0.0/0,address:::/0" \
  --outbound-rules "protocol:tcp,ports:all,address:0.0.0.0/0,address:::/0 protocol:udp,ports:all,address:0.0.0.0/0,address:::/0 protocol:icmp,address:0.0.0.0/0,address:::/0"
```

Outbound stays open. The stack downloads worker packages and sandbox images,
calls model providers and GitHub, and Caddy calls Let's Encrypt.

On the Droplet, as `bastion`:

```sh
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow OpenSSH
sudo ufw allow 443/tcp
sudo ufw enable
sudo ufw status verbose
```

### Step 8: a token for the owner, and the server in the app

The server makes the owner's token on its first start, and keeps it in
`data/token`, readable by its user only (`main.rs`, `token`). For this
service, that file is `~/.local/share/dev.tacoda.bastion/server/token`. The
owner's token is an admin token (`http.rs`, `router`).

Copy it to the Mac's clipboard without printing it:

```sh
ssh -i ~/.ssh/bastion_do bastion@bastion.example.com \
  'cat ~/.local/share/dev.tacoda.bastion/server/token' | pbcopy
```

In the desktop app, open **Settings**, then the **Bastion servers** card.
Fill in the three fields (`crates/bastion-desktop/src/pages/settings/places.rs`):

- **Name:** `work`, or any name you like.
- **URL:** `https://bastion.example.com`
- **Token:** paste it.

Press **Add**. The app keeps the token in the macOS Keychain
(`crates/bastion-client/src/servers.rs`). It refuses an `http://` URL beyond
the machine, because the token would go in the clear.

The owner's token cannot be revoked from the app. To replace it, delete the
file and restart the service. The server makes a new one when the file is
missing (`main.rs`, `token`). Then update the token in the app.

To give someone else access, or to give yourself a revocable token, use the
**People** card. It shows on a remote server, for an admin
(`crates/bastion-desktop/src/pages/settings/people.rs`). Add a person with a
role (admin, member, or viewer), then press **Issue a token**. The server
shows the token once and keeps only its hash (DECISIONS.md D31). Send it to
the person over a channel the employer approves.

### Step 9: model provider keys and `GH_TOKEN`

Put model provider keys in **Settings**, under **Model providers**, with the
work server selected. The server stores each key in the stack's vault, read
only by `llm-router`, and never returns it (DECISIONS.md D34,
`crates/bastion-server/src/api/providers.rs`). Do not put keys in the
service's environment or in any file on the Droplet. A key in the
environment works too (README.md), but it is a file or a unit setting that a
backup or a snapshot copies.

Use work keys on the work server. A personal key on an employer's server
mixes billing and data.

`GH_TOKEN` is not the same. The seeded GitHub steps read `GH_TOKEN` from the
vault (`crates/bastion-server/src/seed_flows.rs`,
`crates/bastion-server/src/factory/shell.rs`). The app has no field that
writes a named secret such as `GH_TOKEN` to the vault today: the server's
routes write only provider keys and compatible-host keys
(`crates/bastion-server/src/http.rs`). See the open questions.

The `gh` sign-in on the host is separate. Doctor checks it, and it is
optional (`doctor.rs`). `gh auth login` keeps its token in `gh`'s own
configuration on disk. If the employer forbids tokens on disk, skip it, and
land changes locally.

### Step 10: backups and updates

What to keep:

| Folder | Holds | Source |
|---|---|---|
| `~/.local/share/dev.tacoda.bastion/server` | The owner's token, `compose.log`, and bundles. | `main.rs`, `scripts/install.sh` |
| `~/.local/lib/bastion-<version>/share/bastion/data` | The stack's database (`bastion.db`: jobs, people, tokens, settings, audit) and the worktrees. | `worker-compose.yaml` (`sqlite:./data/bastion.db`, `worktree_root: data/worktrees`); `docs/engine-findings.md` N12: each compose folder keeps its own `data/` |

Where the `secrets` worker keeps the vault on disk is not known. See the
open questions.

Turn on Droplet backups, and take a snapshot before each update. Backups
cost 20% (weekly) or 30% (daily) of the Droplet's price; snapshots cost
$0.06 per GB per month (https://www.digitalocean.com/pricing/droplets,
"Additional product pricing"):

```sh
doctl compute droplet-action enable-backups <droplet-id> --wait
doctl compute droplet-action snapshot <droplet-id> --snapshot-name "bastion-$(date +%F)" --wait
```

A Droplet backup copies a live disk, and SQLite can be mid-write. For a
clean file copy, stop the service first. This also stops running jobs:

```sh
systemctl --user stop bastion-server
tar -czf ~/bastion-backup-$(date +%F).tar.gz \
  -C ~ .local/share/dev.tacoda.bastion/server \
  .local/lib/bastion-*/share/bastion/data
systemctl --user start bastion-server
```

Then copy the archive to the Mac, and delete it from the Droplet. It holds
the owner's token.

```sh
scp -i ~/.ssh/bastion_do bastion@bastion.example.com:bastion-backup-*.tar.gz ~/Backups/
```

**Updates have a gap. Read this before you run `bastion update`.**

`bastion update` checks the release signature, unpacks the new release into
a new folder `~/.local/lib/bastion-<new>`, and relinks `~/.local/bin`
(`crates/bastion-cli/src/update.rs`). But the service unit names the old
folder by its full path (`scripts/install.sh`). So a restart after
`bastion update` still runs the old version.

Running the installer again with `--service` rewrites the unit for the new
folder. But then `--checkout` points at a new `share/bastion` with an empty
`data/`. The jobs, people, tokens, and settings stay behind in the old
folder.

Until the code fixes this, update like this. It was not tested; take a
snapshot first:

```sh
doctl compute droplet-action snapshot <droplet-id> --snapshot-name "bastion-pre-update-$(date +%F)" --wait
```

On the Droplet:

```sh
old="$(readlink -f ~/.local/bin/bastion-server | sed 's|/bin/bastion-server$||')"
systemctl --user stop bastion-server
curl -fsSL https://tacoda.github.io/bastion/install.sh | sh -s -- --service
systemctl --user stop bastion-server
new="$(readlink -f ~/.local/bin/bastion-server | sed 's|/bin/bastion-server$||')"
echo "old: $old  new: $new"
# If old and new are the same folder, stop here: there is nothing to move.
rm -rf "$new/share/bastion/data"
cp -a "$old/share/bastion/data" "$new/share/bastion/data"
systemctl --user start bastion-server
journalctl --user -u bastion-server -n 50
```

Check in the app that the jobs and people are still there. Keep the old
folder until you have checked.

### Step 11: what can go wrong

| Problem | Fix |
|---|---|
| `/dev/kvm` is missing on the Droplet. | Destroy the Droplet. Use the Mac, or a host from section 1. |
| Doctor says `iii`: "found iii" with another version. | Install 0.25.0 again (step 4). Do not run `iii update`. |
| Doctor says it cannot find `iii`. | Add the `PATH` drop-in (step 5), then restart the service. |
| The sandbox fails with `KVM not accessible`. | The user is not in the `kvm` group. Run `sudo usermod -aG kvm bastion`, then restart the user's session or reboot (https://iii.dev/docs/0-25-0/troubleshooting.md). |
| VMs boot but turns are very slow. | Nested virtualization is slow on Droplets, by DigitalOcean's own answer. Try a larger dedicated-vCPU plan, or move off DigitalOcean. |
| `systemctl --user` fails with "Failed to connect to bus". | Sign in over SSH as `bastion`, not through `sudo su`. |
| The server stops when you sign out. | Run `sudo loginctl enable-linger bastion`. |
| Caddy gets no certificate. | Check that the A record resolves to the Droplet, and that both firewalls allow 443. Read `journalctl -u caddy`. |
| The app says the token is unknown (`401`). | The token is wrong or revoked. Copy the owner's token again (step 8), or issue a new one. |
| The app refuses the URL. | Use `https://`. The app refuses `http://` beyond the machine. |
| The Jobs page does not update live. | Check that Caddy proxies `/events`. Caddy flushes event streams by default; a second proxy or a CDN in front may not. |
| Jobs stopped after a restart. | A server restart stops its stack (D13). Restart only when no job runs. |
| A job cannot push or open a pull request. | `GH_TOKEN` is not in the vault, or `gh` is not signed in. See step 9 and the open questions. |
| After an update, the jobs and people are gone. | The new folder has an empty `data/`. Stop the service, copy `data/` from the old folder (step 10), and start it. |
| The sandbox VMs have no network after `ufw enable`. | Not known whether `ufw` affects the VMs' traffic. Run `sudo ufw disable` for a moment to test, and report what you find. |
| The work and personal accounts mix. | Run `doctl auth switch --context work` and `doctl account get` before each `doctl` command. |

## 4. Open questions

1. **Does `/dev/kvm` work on this Droplet plan, and how fast?** Only a test
   settles it (step 3, and the test job in step 5). DigitalOcean documents
   nothing; its staff say "supported, not recommended".
2. **Does a Bare Metal GPU server expose native KVM, and at what price?**
   Its feature page does not say.
3. **Where does the `secrets` worker keep the vault on disk?** It decides
   what a backup must hold, and whether a snapshot copies the keys.
4. **How does `GH_TOKEN` get into the vault?** The app has no field for a
   named secret. Until it has one, a GitHub step that needs `GH_TOKEN` fails
   on this server.
5. **The update gap.** `bastion update` does not change the service unit,
   and a new install folder starts with an empty `data/`. Should the
   installer keep `--checkout` and `data/` in a stable path, outside the
   versioned folder?
6. **Which other ports do the workers open?** The compose file names only
   49160 and 49161. Check with `sudo ss -tlnp` (step 5).
7. **Does `ufw` affect the sandbox VMs' network?** Not known.
8. **Does the employer allow work code and keys on this Droplet?** Ask
   before step 1.

Try the Droplet for one hour first: steps 1 to 3 only. If `/dev/kvm`
passes, go on. If it fails, delete the Droplet and run the server on the
Mac.
