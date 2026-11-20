<div align="center">
  <img src="packaging/linux/io.github.nandazahran.incuscubby.png" width="128" height="128" alt="IncusCubby logo" />
  <h1>IncusCubby</h1>
  <p>Native Linux desktop GUI (Rust + Slint) for managing local <strong>Incus / LXC</strong> developer sandboxes over <code>/var/lib/incus/unix.socket</code> (<code>GPL-3.0-or-later</code>).</p>
</div>

IncusCubby talks to the local Incus 1.0 REST API over its Unix domain socket, and manages containers, virtual machines, snapshots, workspace bind-mounts, port forwards, storage pools, bridge networks, profiles, and cached images without wrapping the CLI for state or lifecycle operations.

## Features

IncusCubby talks to `incusd` over `/var/lib/incus/unix.socket` using `hyper` and `hyperlocal`, and awaits async operations through `/1.0/operations/{id}/wait`. When the socket is missing or your user lacks permission, the app shows the `Socket unavailable` banner.

While idle, the app polls `/1.0/instances?recursion=2` every three seconds and shows status (`Running`, `Stopped`, `Frozen`), IPv4 address, memory usage and limit, CPU seconds, root disk usage, network RX/TX counters, and process count. It can start, stop, restart, freeze, unfreeze, CoW clone, edit live CPU and memory limits, and delete an instance behind an inline confirmation.

Terminal and IDE launchers:

- Shell opens `incus exec <name> -- su -l root` in your configured terminal emulator, or the first one found in `PATH` from `x-terminal-emulator`, `gnome-terminal`, `konsole`, `ghostty`, `alacritty`, `kitty`, `foot`, `wezterm`, `xfce4-terminal`, and `xterm`.
- IDE opens your configured editor, or the first one found in `PATH` from `codium`, `code`, `zed`, `zeditor`, `kate`, `subl`, `hx`, `nvim`, and `emacs`.
- Workspace mounts attach host directories with `shift: "true"` ID-mapped ownership at `/workspace`, or a custom Incus storage volume. `proxy` devices forward `127.0.0.1:<host_port>` to `127.0.0.1:<container_port>`.
- Package actions run non-interactively through `/1.0/instances/{name}/exec` on Debian and Ubuntu (`apt-get`), Alpine (`apk`), Arch Linux (`pacman`), Fedora and the RHEL family (`dnf`), and openSUSE (`zypper`).

Create accepts `container` or `virtual-machine` from a local or `images.linuxcontainers.org` simplestreams alias, with `cloud-init.user-data`, profiles, and snapshots you can take, restore, or delete. The Advanced views cover storage pools and custom volumes, managed NAT bridge networks and DHCP leases, profiles, instance logs, file push and pull, and locally cached base images.

## Prerequisites & Installation

Download prebuilt Linux binaries from the [releases page](https://github.com/nandazahran/IncusCubby/releases) (no Rust toolchain required):

| Platform | Artifact |
|---|---|
| Linux `x86_64` (AppImage) | `incus-cubby-<version>-x86_64.AppImage` |
| Linux `aarch64` (AppImage) | `incus-cubby-<version>-aarch64.AppImage` |
| Linux `x86_64` (tarball) | `incus-cubby-<version>-x86_64-linux.tar.gz` |
| Linux `aarch64` (tarball) | `incus-cubby-<version>-aarch64-linux.tar.gz` |
| Debian / Ubuntu `amd64` (`.deb`) | `incus-cubby_<version>_amd64.deb` |
| Debian / Ubuntu `arm64` (`.deb`) | `incus-cubby_<version>_arm64.deb` |
| Fedora / RHEL / openSUSE `x86_64` (`.rpm`) | `incus-cubby-<version>-1.x86_64.rpm` |
| Fedora / RHEL / openSUSE `aarch64` (`.rpm`) | `incus-cubby-<version>-1.aarch64.rpm` |
| Checksums | `SHA256SUMS.txt` |

Verify downloaded artifacts against `SHA256SUMS.txt` before installing. Arch/CachyOS users can also build locally from [`packaging/linux/PKGBUILD`](packaging/linux/PKGBUILD).

### Runtime prerequisites

- **Incus daemon** running with `/var/lib/incus/unix.socket` accessible to your user:
  ```bash
  sudo usermod -aG incus-admin "$USER"
  # Log out and back in for the group membership to take effect
  ```
- **`incus` CLI** in `PATH` (only required for the *Shell* button, which spawns `incus exec <name> -- su -l root`).

### Building from source

Requires **Rust 1.91+** (`edition = "2024"`).

```bash
git clone https://github.com/nandazahran/IncusCubby.git
cd IncusCubby
cargo run --release
```

Linux system dependencies, repository layout, test commands, and packaging steps are covered in [CONTRIBUTING.md](CONTRIBUTING.md).

## Usage

1. Ensure `incusd` is running and your user belongs to `incus-admin`.
2. Launch `cargo run --release` (or the compiled `target/release/incus-cubby` binary).
3. Switch between **Simple** and **Advanced** UI modes (and **Dark** / **Light** themes) in the header, or navigate the Advanced views:
   - **Sandboxes**: Select an instance to inspect telemetry, start/stop/restart/freeze, open a shell or IDE, clone, edit CPU/RAM limits, upgrade/install packages, manage disk mounts and proxy ports, take/restore snapshots, view logs, or push/pull files.
   - **Create**: Switch between **Official Images (LinuxContainers)** (`container` or `virtual-machine`, presets or fetched `images.linuxcontainers.org` aliases, CPU/memory/profiles, `/workspace` bind-mount, `cloud-init.user-data`) and **Community Scripts (community-scripts/Incus)** (search/filter catalog, preview `ct`/`install` scripts, deploy new containers or run in existing ones).
   - **Storage & Net**: View pool capacity, create/delete custom volumes, create/delete managed bridge networks (with IPv4/IPv6 NAT toggle), and inspect DHCP leases.
   - **Profiles**: Create, update, or delete non-`default` Incus profiles with CPU and memory limits.
   - **Images**: Inspect and delete locally cached Incus base images.
   - **Settings / About / Legal**: Persist preferred terminal emulator, preferred/custom IDE, UI mode (`simple_mode`), and theme (`light_theme`) to `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`), and view license and attribution notices.

## Documentation

- [Architecture Specification](docs/ARCHITECTURE.md)
- [Incus REST API & Simplestreams Reference](docs/INCUS_API.md)
- [Roadmap, risks, and decisions](docs/ROADMAP.md)
- [Privacy Policy](docs/PRIVACY.md): local-only operation, no telemetry.

## Prior Art & References

IncusCubby is an independent Rust + Slint implementation under the GNU General Public License v3.0 or later (`LICENSE`). Inspiration and references consulted:

- [Incus](https://linuxcontainers.org/incus/): system container and virtual machine manager (`1.0` Unix socket REST API and Web UI workflows).
- [DistroShelf](https://github.com/ranfdev/DistroShelf) and [BoxBuddyRS](https://github.com/Dvlv/BoxBuddyRS): Linux desktop container manager UX workflows (clone, package upgrade and install, terminal and IDE integration).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for building from source, the repository layout, live smoke testing, and the checks enforced locally.

## License

Licensed under the [GNU General Public License v3.0 or later (`GPL-3.0-or-later`)](LICENSE).
