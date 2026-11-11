<!-- voice-register: informational -->
<!-- voice-english: ste -->

# Hosting a Bastion server on Hetzner

This plan puts `bastion-server` on a Hetzner dedicated server with Ubuntu,
behind Caddy for TLS, so the desktop app on your Mac can add it as a remote
server. The deciding constraint is `/dev/kvm`: every agent turn runs in a
sandbox micro-VM (DECISIONS.md D14), and Hetzner Cloud servers have no
nested virtualization. So you need a dedicated server. The cheapest new one
today is the AX41 at €59.00 a month, in Germany or Finland only. Some facts
are still open, and the last section lists them.

Each web fact carries its source, and all date from 2026-11-11.

## Before you start

- You need a domain name whose DNS you control. This plan uses
  `bastion.example.com`. Put your own name wherever that one appears.
- `SERVER_IP` means the IPv4 address Hetzner gives the server.
- The Linux user that runs Bastion is `bastion`.
- Do not use `ps aux` or `pgrep -fl` to look at the server's processes. The
  command lines of iii's VMs can carry host secrets. Use `ss -tlnp` and
  `systemctl` instead.

## 1. Account and machine

### Why a Cloud VPS cannot run the sandbox

Hetzner's own FAQ answers "Can I run virtual machines on cloud servers, or
rather is nested virtualization possible?" with "No, this is not possible on
cloud server."
(https://docs.hetzner.com/cloud/servers/faq). A third party tested every
Cloud line in May 2026 and found `/dev/kvm` absent on CCX, CPX and CAX
(https://mvm-deployment-docs.pages.dev/providers/hetzner/). So a CX, CPX,
CAX or CCX server cannot run Bastion's sandbox. Doctor refuses the stack
there with "Sandbox VMs: no /dev/kvm" (`crates/bastion-server/src/doctor.rs`).

A dedicated server is bare metal, so the CPU's own virtualization is there.

### The machine to order

Order an **AX41**. Hetzner lists it at "€ 59.00 per month, excl. VAT", with
"€ 0.00 one-time setup fee", an AMD Ryzen 5 3600, 64 GB DDR4 and 2x 512 GB
NVMe, in data centers FSN1 and HEL1
(https://www.hetzner.com/dedicated-rootserver/ax41). Hetzner's AX overview
lists the Ryzen 5 3600 "with virtualization support"
(https://www.hetzner.com/dedicated-rootserver/ax41-nvme).

The AX41 is large enough. The stack allows 8 sandbox VMs at once, each with
1 CPU and 1024 MB (`worker-compose.yaml`, `iii-sandbox`). That is 8 GB and 8
threads at most, and the Ryzen 5 3600 has 6 cores and 12 threads.

Check the Server Auction before you order. It sells used dedicated servers
with no setup fee (https://www.hetzner.com/sb). On 2026-11-11 that page
said "from € 75.70 per month, excl. VAT", which is more than the AX41.
Auction stock and prices change daily, so compare on the day you order.
Any auction server works if its CPU supports virtualization.

### Location

Hetzner sells dedicated servers only in Germany and Finland. Its docs say
"Ashburn, VA, Hillsboro, OR, and Singapore are currently only available for
cloud products" and "you cannot order dedicated root servers (Hetzner Robot
account)" there (https://docs.hetzner.com/cloud/general/locations/). So a
server near Austin is not possible on Hetzner. Pick FSN1 or HEL1. The
latency from Texas to either one is not measured here.

### Steps

1. Create a Hetzner account at https://www.hetzner.com.
2. Dedicated servers are managed in Robot (https://robot.hetzner.com), not
   in a Cloud Console project. You do not need a Cloud project.
3. On your Mac, make an SSH key if you have none:

   ```sh
   ssh-keygen -t ed25519 -C "bastion-hetzner"
   cat ~/.ssh/id_ed25519.pub
   ```

4. Add the public key in Robot, and choose it when you order.
5. Order the AX41 in FSN1 or HEL1.

## 2. Ubuntu and first hardening

Install **Ubuntu 24.04**. The release builds its Linux binaries on
`ubuntu-24.04` runners (`.github/workflows/release.yml`), so an older
Ubuntu can lack the C library version the binaries need.

1. In Robot, turn on the rescue system with your SSH key, then reset the
   server.
2. Log in to the rescue system and start the installer. Hetzner's docs say
   to "type installimage to start the installimage script"
   (https://docs.hetzner.com/robot/dedicated-server/operating-systems/installimage/).

   ```sh
   ssh root@SERVER_IP
   installimage
   ```

3. Pick the Ubuntu 24.04 image from the menu. Keep the default disk layout.
   Save, let it install, then run `reboot`.
4. Log in as root on the new system. Remove the old host key on your Mac
   first, because the rescue system had a different one.

   ```sh
   ssh-keygen -R SERVER_IP
   ssh root@SERVER_IP
   ```

5. Update the system and add the user that runs Bastion. The `kvm` group
   gives that user access to `/dev/kvm`.

   ```sh
   apt update && apt -y full-upgrade
   adduser --disabled-password --gecos "" bastion
   usermod -aG sudo,kvm bastion
   passwd bastion
   mkdir -p /home/bastion/.ssh
   cp /root/.ssh/authorized_keys /home/bastion/.ssh/
   chown -R bastion:bastion /home/bastion/.ssh
   chmod 700 /home/bastion/.ssh && chmod 600 /home/bastion/.ssh/authorized_keys
   ```

   The password is for `sudo` only. SSH will not accept it.

6. Allow SSH keys only, and refuse root logins. sshd keeps the first value
   it reads, so the file name starts with `00-`.

   ```sh
   cat > /etc/ssh/sshd_config.d/00-hardening.conf <<'EOF'
   PasswordAuthentication no
   KbdInteractiveAuthentication no
   PermitRootLogin no
   EOF
   sshd -t && systemctl restart ssh
   ```

7. Open a second terminal on your Mac and log in as `bastion` before you
   close the root session. If it fails, fix it from the root session.

   ```sh
   ssh bastion@SERVER_IP
   ```

8. Turn on unattended security upgrades.

   ```sh
   sudo apt install -y unattended-upgrades
   sudo dpkg-reconfigure -plow unattended-upgrades
   ```

   Unattended upgrades do not reboot the server by default. A kernel update
   needs a reboot, and a reboot stops running jobs, so reboot at a time you
   choose.

9. Reboot once now, so the `kvm` group applies to every session.

   ```sh
   sudo reboot
   ```

## 3. Check virtualization first

Do this before you install anything for Bastion. If a check fails, stop
here, because Doctor will refuse the stack.

```sh
ssh bastion@SERVER_IP
grep -Ec '(vmx|svm)' /proc/cpuinfo   # more than 0: the CPU has virtualization
ls -l /dev/kvm                        # expect: crw-rw---- ... root kvm ...
id -nG                                # expect kvm in the list
sudo apt install -y cpu-checker && kvm-ok
test -r /dev/kvm && test -w /dev/kvm && echo "kvm usable"
```

Doctor checks only that `/dev/kvm` exists. iii's sandbox also needs read
and write access to it. Without access, a worker fails with "KVM not
accessible", and the fix is `sudo usermod -aG kvm $USER` and a new session
(https://iii.dev/docs/troubleshooting). Step 2 already did this.

## 4. Dependencies

The stack needs three things on the host: `iii` 0.25.0, `git`, and outbound
network access. It does not need Docker. The sandbox runs micro-VMs through
`/dev/kvm`, and iii's docs say a sandboxed worker needs "no containers or
Dockerfiles" (https://iii.dev/docs/how-to/developing-sandbox-workers). The
compose file in this repository names no container runtime. It also needs
no Rust: the release replaces `cargo run` for `bastion-openai-compat` with a
built binary (`scripts/package.sh`).

| Program | Why | Required |
|---|---|---|
| `iii` 0.25.0 | Doctor requires `iii --version` to print exactly `0.25.0`. The server runs `iii compose` and `iii kill`. | Yes |
| `git` | The server pushes branches with the owner's own git, and reads `git config user.name` (`forge.rs`, `main.rs`). | Yes |
| `curl` | The installers. | Yes |
| `minisign` | `install.sh` checks the release signature when it is present. | Recommended |
| `sqlite3` | Online backups of the stack's database (step 10). | Recommended |
| `gh` | Pull requests, CI checks and issues. Doctor's GitHub check is optional, and landing locally works without it. | See step 9 |

1. Install the system packages.

   ```sh
   sudo apt install -y git curl sqlite3 minisign
   ```

   If apt has no `minisign`, skip it. The installer then checks only the
   checksums, and says so.

2. Install iii 0.25.0. iii's docs give the version as an argument
   (https://docs.iii.dev/docs/install.md). Use the argument form: in
   `VERSION=0.25.0 curl ... | sh`, the variable goes to `curl`, not to `sh`.

   ```sh
   curl -fsSL https://install.iii.dev/iii/main/install.sh | sh -s -- 0.25.0
   ~/.local/bin/iii --version   # expect: 0.25.0
   ```

   The binary goes to `~/.local/bin` by default (same source).

3. Give the systemd user service a `PATH` that has `~/.local/bin`. The
   server finds `iii` on its own `PATH` and passes `PATH` to what it runs
   (`crates/bastion-server/src/machine.rs`). Do this before step 5, because
   the installer starts the service at once.

   ```sh
   mkdir -p ~/.config/systemd/user/bastion-server.service.d
   cat > ~/.config/systemd/user/bastion-server.service.d/path.conf <<'EOF'
   [Service]
   Environment=PATH=%h/.local/bin:/usr/local/bin:/usr/bin:/bin
   EOF
   ```

4. Set the git name and email. The server names its owner, who is its
   admin, from `git config user.name`, or "owner" when it is unset.

   ```sh
   git config --global user.name "Ian Johnson"
   git config --global user.email "you@example.com"
   ```

## 5. Install bastion-server as a user service

1. Turn on lingering first, so the user service runs without an open SSH
   session and starts at boot.

   ```sh
   sudo loginctl enable-linger bastion
   ```

2. Run the installer from the README with `--service`.

   ```sh
   curl -fsSL https://tacoda.github.io/bastion/install.sh | sh -s -- --service
   ```

   Add `--version vX.Y.Z` to pin a release. The installer writes
   `~/.config/systemd/user/bastion-server.service` with this command:

   ```text
   ~/.local/lib/bastion-<v>/bin/bastion-server --checkout ~/.local/lib/bastion-<v>/share/bastion
     --data ~/.local/share/dev.tacoda.bastion/server --port 49190 --engine ws://127.0.0.1:49160
   ```

   The unit has no `--listen`, so the server listens on `127.0.0.1:49190`
   only. That is correct for this plan: Caddy takes the public traffic.

3. Check the service and its stack.

   ```sh
   systemctl --user status bastion-server
   journalctl --user -u bastion-server -n 50
   tail -n 50 ~/.local/share/dev.tacoda.bastion/server/compose.log
   ```

   Expect `bastion-server: serving http on 127.0.0.1:49190`. If the log
   says `stack did not start`, Doctor refused it. The Engine page in the
   app shows which check failed, after step 8.

Run `systemctl --user` only in an SSH session where you logged in as
`bastion`. In a `sudo su bastion` shell it fails, because that shell has
no user bus.

## 6. DNS name and TLS

### The decision: Caddy in front, not the server's own TLS

The server can serve TLS itself:
`--listen IP --tls-cert PEM --tls-key PEM` (`crates/bastion-server/src/tls.rs`,
`main.rs`). It reads the certificate once, when it starts. It has no reload.
So each Let's Encrypt renewal needs a server restart. A restart stops the
stack too (D13), and that stops every running agent turn.

Caddy renews its certificates in the background, with no Bastion restart.
Caddy flushes a `text/event-stream` response at once
(https://caddyserver.com/docs/caddyfile/directives/reverse_proxy), so the
server's event stream (D29) passes through it. The server stays on
loopback, and Caddy is the only thing on the public port.

### Steps

1. At your DNS provider, add an A record: `bastion.example.com` to
   `SERVER_IP`. Wait until it resolves.

   ```sh
   dig +short bastion.example.com   # expect SERVER_IP
   ```

2. Install Caddy from its official repository
   (https://caddyserver.com/docs/install).

   ```sh
   sudo apt install --yes debian-keyring debian-archive-keyring apt-transport-https curl
   curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | sudo gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
   curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | sudo tee /etc/apt/sources.list.d/caddy-stable.list
   sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg
   sudo chmod o+r /etc/apt/sources.list.d/caddy-stable.list
   sudo apt update
   sudo apt install caddy
   ```

3. Point Caddy at the server.

   ```sh
   sudo tee /etc/caddy/Caddyfile >/dev/null <<'EOF'
   bastion.example.com {
   	reverse_proxy 127.0.0.1:49190
   }
   EOF
   sudo systemctl reload caddy
   ```

4. Do step 7 (the firewall) now, so port 443 is open, then check the
   certificate.

   ```sh
   sudo journalctl -u caddy -n 50 --no-pager
   curl -s -o /dev/null -w '%{http_code}\n' https://bastion.example.com/people   # expect 401: no token
   ```

Caddy gets the certificate through the TLS-ALPN challenge "over port
`443`", or through the HTTP challenge "over port `80`"
(https://caddyserver.com/docs/automatic-https). Port 443 is open, so port
80 stays closed. If the first certificate fails, open port 80 for a
moment and read Caddy's log.

The desktop app trusts the Mac's own root certificates (`reqwest` with
`rustls-tls-native-roots` in `crates/bastion-client/Cargo.toml`). macOS
trusts Let's Encrypt, so the app needs no extra setup.

## 7. Firewall

### Ports

| Port | Who listens | State |
|---|---|---|
| 22/tcp | sshd | Open |
| 443/tcp | Caddy, HTTPS to the server | Open |
| 80/tcp | Caddy, HTTP challenge and redirect | Closed (see step 6) |
| 49190/tcp | `bastion-server`, plain HTTP on `127.0.0.1` | Closed |
| 49160/tcp | iii engine, `iii-worker-manager` on `127.0.0.1` | Closed |
| 49161/tcp | iii engine, the governed listener on `127.0.0.1` | Closed |

The engine must never reach the internet. `worker-compose.yaml` says it
"trusts the worker name a connection declares", so anyone who reaches it
can act as any worker. The compose file also names a port 49150 for
Bastion; its use is an open question. Keep it closed.

### ufw

```sh
sudo ufw default deny incoming
sudo ufw default allow outgoing
sudo ufw allow OpenSSH
sudo ufw allow 443/tcp
sudo ufw enable
sudo ufw status verbose
sudo ss -tlnp   # every port except 22 and 443 must show 127.0.0.1 or ::1
```

Allow outgoing traffic. The stack downloads iii packages and sandbox images
(`auto_install: true`, and the `browser` image from `mcr.microsoft.com`),
and agents call model providers.

### Hetzner's firewall

Hetzner Cloud Firewalls apply to Cloud servers only. A dedicated server has
the Robot firewall, which Hetzner configures "to the switch port", under
"Main functions" -> "Servers" -> "Firewall"
(https://docs.hetzner.com/robot/dedicated-server/firewall/). It is a
"stateless firewall" with "a maximum of 10 rules per direction" (same
source). A stateless firewall needs explicit rules for return traffic.
Hetzner's example for TCP replies is "Destination port: 32768-65535
(Ephemeral Port Range), Protocol: tcp, TCP flags: ack, Action: accept".

If you turn it on, use these incoming rules:

1. TCP, destination port 22, accept.
2. TCP, destination port 443, accept.
3. TCP, destination port 32768-65535, TCP flags `ack`, accept.

Then test DNS and downloads, because UDP replies are not in Hetzner's
example. If they fail, turn the Robot firewall off and rely on ufw.

```sh
getent hosts github.com && curl -sI https://github.com | head -1
```

## 8. Your token, and the server in the app

The server makes the owner's token on its first start. It keeps the token
in `data/token`, readable by the `bastion` user only (`main.rs`). That
token is the owner's, and the owner is the admin (`access.rs`).

1. On your Mac, copy the token to the clipboard. It never shows on screen.

   ```sh
   ssh bastion@bastion.example.com cat .local/share/dev.tacoda.bastion/server/token | pbcopy
   ```

2. In the app, open **Settings**. In **Bastion servers**, fill in:
   - **Name**: a short name such as `hetzner`. The name `local` is taken.
   - **URL**: `https://bastion.example.com`. The app refuses an `http://`
     URL beyond this machine (`crates/bastion-client/src/servers.rs`).
   - **Token**: paste it.
3. Click **Add**. The app keeps the token in the macOS keychain (D30).
4. Pick the new server at the top of the app. Open **Engine** and read
   Doctor.

To give another person access, add them in **Settings**, **People**, then
click **Issue a token**. The server shows a token once and keeps only its
hash (D31). Revoke it in the same place.

## 9. Keys go into the vault from the app

1. With the remote server picked, open **Settings**, **Model providers**,
   and add a key. The server stores it in the stack's `secrets` vault, for
   `llm-router` only (D34, `crates/bastion-server/src/api/providers.rs`).
2. Do not put a provider key in the server's environment, the unit file, a
   shell profile, or any other file.
3. Read Doctor again. "Model providers" must pass.

`GH_TOKEN` is not settled yet. A shell step that names `GH_TOKEN` reads it
from the vault (`crates/bastion-server/src/factory/shell.rs`). This tree
has no page or route that writes a secret other than a provider key into
the vault. Until one exists, do not work around it by writing `GH_TOKEN`
into a file. See the open questions.

Host `gh` is a second, separate need. Pull requests go through the
`github` worker's `gh`, and pushes go through the host's own git
(`crates/bastion-server/src/forge.rs`). On a server with no keyring,
`gh auth login` keeps its token in a file in `~/.config/gh`. That conflicts
with "never in files". Landing locally works without `gh`, so leave it out
until you decide.

A workspace is a repository on the server's disk. Clone each repository
on the server, then add it in **Settings**, **Workspaces**, with its path
on the server.

## 10. Backups and updates

### What to back up

The server's records live in two folders:

- `~/.local/share/dev.tacoda.bastion/server`: the owner's token, the audit
  chain's key (`audit-key`), the audit spool and anchor, and `compose.log`.
- `<checkout>/data`: the stack's database `bastion.db` and the job
  worktrees. The compose file sets `sqlite:./data/bastion.db` and
  `worktree_root: data/worktrees`, relative to the checkout.

The checkout is inside the versioned install folder
(`~/.local/lib/bastion-<v>/share/bastion`). That matters for updates.

The audit key signs the audit chain (D21). If you lose it, you cannot check
the chain. Keep backups encrypted, or on a disk only you can read.

```sh
share="$(sed -n 's/.*--checkout \([^ ]*\).*/\1/p' ~/.config/systemd/user/bastion-server.service)"
stamp="$(date +%Y%m%d-%H%M%S)"
mkdir -p ~/backups/"$stamp"
sqlite3 "$share/data/bastion.db" ".backup '$HOME/backups/$stamp/bastion.db'"
tar -czf ~/backups/"$stamp"/stack-data.tar.gz -C "$share" --exclude='data/bastion.db*' data
tar -czf ~/backups/"$stamp"/server.tar.gz -C ~/.local/share/dev.tacoda.bastion server
chmod -R go-rwx ~/backups
```

`sqlite3 .backup` copies a consistent database while the stack runs. Then
copy the backups off the machine, from your Mac:

```sh
rsync -a bastion@bastion.example.com:backups/ ~/bastion-backups/
```

The vault's own storage location is an open question, so a restore may
need the provider keys again. Enter them in **Settings**.

### Updating

`bastion update` unpacks the new release beside the old one and moves the
links in `~/.local/bin` (`crates/bastion-cli/src/update.rs`). It does not
change the unit, which still names the old folder. The stack's data stays
in the old folder too. So follow these steps in order:

```sh
systemctl --user stop bastion-server
old="$(sed -n 's/^ExecStart=\(.*\)\/bin\/bastion-server .*/\1/p' ~/.config/systemd/user/bastion-server.service)"
~/.local/bin/bastion update
new="$(dirname "$(dirname "$(readlink -f ~/.local/bin/bastion-server)")")"
echo "$old -> $new"
cp -a "$old/share/bastion/data" "$new/share/bastion/"
sed -i "s|$old|$new|g" ~/.config/systemd/user/bastion-server.service
systemctl --user daemon-reload
systemctl --user start bastion-server
```

The stop ends running jobs, because the stack stops with the server (D13).
Do not re-run `install.sh --service` to update. It starts the new version
at once, with an empty `data` folder, before you can copy the old one.

If the new release pins another iii version, Doctor says so. Install that
version with step 4.2.

Keep the old folder until the new version runs well, then remove it.

## 11. What can go wrong

| What you see | Cause | Fix |
|---|---|---|
| Doctor: "Sandbox VMs: no /dev/kvm" | The machine has no KVM, for example a Hetzner Cloud server. | Use a dedicated server (step 1). |
| "KVM not accessible" | The user is not in the `kvm` group, or the session started before the change. | `sudo usermod -aG kvm bastion`, then `sudo reboot`. |
| Doctor: "iii" fails | `iii` is not on the service's `PATH`, or the version is not `0.25.0`. | Check step 4.3, then `systemctl --user daemon-reload` and restart. |
| `Failed to connect to bus` | `systemctl --user` ran in a `sudo su` shell. | Log in over SSH as `bastion`. |
| The server stops when you log out | Lingering is off. | `sudo loginctl enable-linger bastion`. |
| Caddy has no certificate | DNS does not point at the server, or port 443 is blocked. | Check `dig`, ufw and the Robot firewall. Read `journalctl -u caddy`. |
| The app says the URL "sends the token in the clear" | The URL starts with `http://`. | Use `https://bastion.example.com`. |
| 401 "no valid token" | The token is wrong, or it was revoked. | Copy the owner's token again (step 8). |
| Jobs and rules are gone after an update | The service runs from the new folder with an empty `data`. | Stop the service, copy `data` from the old folder, start it. |
| A C library version error at start | The Ubuntu release is older than 24.04. | Reinstall with Ubuntu 24.04. |
| A running job stopped | The server restarted, and the stack went down with it (D13). | Restart only between jobs. Reboot for kernel updates at a time you choose. |

## Open questions

1. **iii's other ports.** This plan knows 49160 and 49161 from
   `worker-compose.yaml`. It does not know if iii 0.25.0 or a worker opens
   another port, or on which address. Run `sudo ss -tlnp` after the first
   start. ufw blocks any port it finds anyway.
2. **Port 49150.** The compose file names it as Bastion's port. No code in
   this tree shows what uses it.
3. **`GH_TOKEN` in the vault.** The factory reads it from the vault, but
   nothing in the app or the API writes it there.
4. **Host `gh` sign-in.** On a headless server, `gh` keeps its token in a
   file. Decide if that is allowed, or what replaces it.
5. **Where the vault stores secrets.** If it is outside `data/`, the
   backup and the update steps miss it.
6. **Data inside the versioned folder.** The stack's database lives in
   `~/.local/lib/bastion-<v>/share/bastion/data`, and the unit names a
   versioned path. `bastion update` handles neither. A stable data path
   outside `lib`, or an update that moves `data` and rewrites the unit,
   would remove the manual steps in step 10.
7. **Ubuntu 26.04.** The release builds on 24.04. A newer Ubuntu likely
   runs it, but nobody has tested it.
8. **Robot firewall and UDP.** Hetzner's example covers TCP replies only.
   DNS replies over UDP may need a rule.
9. **Latency.** Dedicated servers are in Germany and Finland only. Nobody
   has measured how the app feels from Texas.
10. **Server Auction prices.** They change daily. The price above is one
    reading on 2026-11-11.
11. **The owner in People.** The owner's token works before the database
    opens. This plan did not check whether the owner also shows in the
    People list, or can issue a token to themselves.
