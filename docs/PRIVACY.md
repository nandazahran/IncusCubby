# Privacy Policy

IncusCubby collects no data. Ever.

## What this means

- **No data leaves your machine for tracking.** The app has no backend servers, no analytics, no telemetry, no crash reporting, and no advertising.
- **No accounts, no tracking.** There is nothing to sign up for and nothing to identify you.
- **No third-party SDKs.** No analytics or telemetry library receives anything from the app.

## Network use

IncusCubby is a local desktop manager for your local Incus daemon:

- **Local Unix socket**: All instance, snapshot, device, log, file, storage, network, and profile operations communicate solely with `/var/lib/incus/unix.socket` on your machine.
- **Optional upstream image alias fetch**: Only when you explicitly click *Fetch Remote Aliases* in the *Create* tab does IncusCubby make an HTTPS `GET` request to `https://images.linuxcontainers.org/streams/v1/images.json` to list available public image aliases.
- **Image pulls by Incus**: When you create an instance from a remote alias, the local `incusd` daemon pulls the requested image from `https://images.linuxcontainers.org`.

## System access

- **Incus socket (`/var/lib/incus/unix.socket`)**: Required via `incus-admin` group membership to query and manage your local containers and VMs.
- **Local processes & files**: Spawns your chosen terminal emulator (`incus exec <name> -- su -l root`) or IDE (`codium`/`code`, `zed`/`zeditor`), and reads/writes only the host directories or files you explicitly select for bind-mounts or file push/pull.

## Data stored on your device

IncusCubby saves only your local UI preferences (`preferred_terminal` and `preferred_ide`) in:

- `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`)

Deleting that file removes all state stored directly by IncusCubby.

## Verify it yourself

IncusCubby is free software under the GNU General Public License v3.0 or later (`GPL-3.0-or-later`, `LICENSE`). You do not have to trust this page — read the code at [github.com/nandazahran/IncusCubby](https://github.com/nandazahran/IncusCubby) or build it yourself.

## Changes

If a future version ever changed any of this, it would be announced in the release notes and this page would be updated before that version shipped.
