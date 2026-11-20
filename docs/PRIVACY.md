# Privacy Policy

IncusCubby collects no data.

## What this means

The app has no backend servers, no analytics, no telemetry, no crash reporting, and no advertising, so nothing leaves your machine for tracking. There are no accounts to create and nothing that identifies you. No analytics or telemetry library receives anything from the app.

## Network use

IncusCubby is a local desktop manager for your local Incus daemon:

- Instance, snapshot, device, log, file, storage, network, and profile operations talk only to `/var/lib/incus/unix.socket` on your machine.
- At startup the app fetches the catalog index from `https://community-scripts.org/`. Previewing or deploying a script fetches sources from `https://api.github.com/repos/community-scripts/{Incus,DevScripts}` and `https://raw.githubusercontent.com/community-scripts/{Incus,DevScripts,ProxmoxVE,core}`.
- Distro and app icons load over HTTPS from `https://cdn.jsdelivr.net/gh/selfhst/icons@main/webp/*` and from logo URLs in the Community Scripts catalog, then cache on disk.
- Clicking *Fetch remote aliases* in the *Create* tab makes one HTTPS `GET` to `https://images.linuxcontainers.org/streams/v1/images.json`.
- When you create an instance from a remote alias, the local `incusd` daemon pulls the image from `https://images.linuxcontainers.org`.

## System access

- `/var/lib/incus/unix.socket` requires `incus-admin` group membership and is used to query and manage local containers and VMs.
- IncusCubby spawns your terminal emulator (`incus exec <name> -- su -l root`) or IDE (auto-detected `codium`, `code`, `zed`, `zeditor`, `kate`, `subl`, `hx`, `nvim`, `emacs`, or your configured command), and reads and writes only the host directories or files you select for bind-mounts, push, or pull.

## Data stored on your device

IncusCubby stores only local UI preferences (`preferred_terminal`, `preferred_ide`, `simple_mode`, and `light_theme`) and cached distro/app logos on your device:

- `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`)
- `$XDG_CACHE_HOME/incus-cubby/logos/` (or `~/.cache/incus-cubby/logos/`)

Deleting those paths removes all state stored directly by IncusCubby.

## Verify it yourself

IncusCubby is free software under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`, `LICENSE`). You do not have to trust this page; read the code at [github.com/nandazahran/IncusCubby](https://github.com/nandazahran/IncusCubby) or build it yourself.

## Changes

If a future version changes any of this, the release notes and this page will be updated before that version ships.
