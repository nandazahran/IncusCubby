# Privacy Policy

IncusCubby collects no data. Ever.

## What this means

- **No data leaves your machine for tracking.** The app has no backend servers, no analytics, no telemetry, no crash reporting, and no advertising.
- **No accounts, no tracking.** There is nothing to sign up for and nothing to identify you.
- **No third-party SDKs.** No analytics or telemetry library receives anything from the app.

## Network use

IncusCubby is a local desktop manager for your local Incus daemon:

- **Local Unix socket**: All instance, snapshot, device, log, file, storage, network, and profile operations communicate solely with `/var/lib/incus/unix.socket` on your machine.
- **Community Scripts catalog & script fetches**: On startup, IncusCubby fetches the catalog index from `https://community-scripts.org/`. When you preview or deploy a script, it fetches script sources from `https://api.github.com/repos/community-scripts/{Incus,DevScripts}` and `https://raw.githubusercontent.com/community-scripts/{Incus,DevScripts,ProxmoxVE,core}`.
- **Dynamic OS & app logos**: Fetches distro and app icon assets over HTTPS from `https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/*` and HTTPS logo URLs provided by the Community Scripts catalog, caching them locally on disk.
- **Optional upstream image alias fetch**: Only when you explicitly click *Fetch Remote Aliases* in the *Create* tab does IncusCubby make an HTTPS `GET` request to `https://images.linuxcontainers.org/streams/v1/images.json` to list available public image aliases.
- **Image pulls by Incus**: When you create an instance from a remote alias, the local `incusd` daemon pulls the requested image from `https://images.linuxcontainers.org`.

## System access

- **Incus socket (`/var/lib/incus/unix.socket`)**: Required via `incus-admin` group membership to query and manage your local containers and VMs.
- **Local processes & files**: Spawns your chosen terminal emulator (`incus exec <name> -- su -l root`) or IDE (auto-detected `codium`, `code`, `zed`, `zeditor`, `kate`, `subl`, `hx`, `nvim`, `emacs`, or your configured custom command), and reads/writes only the host directories or files you explicitly select for bind-mounts or file push/pull.

## Data stored on your device

IncusCubby stores only local UI preferences (`preferred_terminal`, `preferred_ide`, `simple_mode`, and `light_theme`) and cached distro/app logos on your device:

- `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`)
- `$XDG_CACHE_HOME/incus-cubby/logos/` (or `~/.cache/incus-cubby/logos/`)

Deleting those paths removes all state stored directly by IncusCubby.

## Verify it yourself

IncusCubby is free software under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`, `LICENSE`). You do not have to trust this page — read the code at [github.com/nandazahran/IncusCubby](https://github.com/nandazahran/IncusCubby) or build it yourself.

## Changes

If a future version ever changed any of this, it would be announced in the release notes and this page would be updated before that version shipped.
