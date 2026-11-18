<div align="center">
  <h1>IncusCubby</h1>
  <p>Native Linux desktop GUI (Rust + Slint) for managing local <strong>Incus / LXC</strong> developer sandboxes over <code>/var/lib/incus/unix.socket</code> (<code>GPL-3.0</code>).</p>
</div>

IncusCubby talks directly to the local Incus 1.0 REST API over its Unix domain socket — managing containers, virtual machines, snapshots, workspace bind-mounts, port forwards, storage pools, bridge networks, profiles, and cached images without wrapping the CLI for state or lifecycle operations.

## Features

- **Direct Unix Socket Client**: Communicates with `incusd` over `/var/lib/incus/unix.socket` using `hyper` + `hyperlocal` with async `/1.0/operations/{id}/wait` handling and automatic `Incus Socket Warning` diagnostics if permissions or the daemon are missing.
- **Live Telemetry & Lifecycle**: Polls `/1.0/instances?recursion=2` every 3 seconds when idle, showing status (`Running`, `Stopped`, `Frozen`), IPv4 address, memory usage/limits, CPU seconds, root disk usage, network RX/TX counters, and process counts. Supports `Start`, `Stop`, `Restart`, `Freeze`/`Unfreeze`, CoW `Clone`, live CPU/memory limit edits, and inline-confirmed `Delete`.
- **Developer Workflow Integration**:
  - **Shell & IDE Launchers**: Opens an interactive root shell (`incus exec <name> -- su -l root`) in your configured or auto-detected terminal emulator (`alacritty`, `kitty`, `foot`, `wezterm`, `konsole`, `gnome-terminal`, `xfce4-terminal`, `xterm`, `x-terminal-emulator`), and opens mounted workspaces in **VSCodium/VS Code** or **Zed**.
  - **Workspace & Custom Disk Mounts**: Attaches host directories with `shift: "true"` ID-mapped ownership (default `/workspace`) or custom Incus storage volumes, plus `proxy` port-forwarding devices (`127.0.0.1:<host_port>` to `127.0.0.1:<container_port>`).
  - **OS-Aware Package Actions**: Runs non-interactive package upgrades and validated package installs via `/1.0/instances/{name}/exec` for Debian/Ubuntu (`apt-get`), Alpine (`apk`), Arch Linux (`pacman`), Fedora/RHEL-family (`dnf`), and openSUSE (`zypper`).
- **Containers, VMs, Cloud-Init & Snapshots**: Create `container` or `virtual-machine` instances from local or `images.linuxcontainers.org` simplestreams aliases, pass `cloud-init.user-data`, apply profiles, and take/restore/delete snapshots.
- **Local Infrastructure Tabs**: Inspect storage pools and create/delete/attach custom filesystem volumes, create/delete managed NAT bridge networks and inspect DHCP leases, manage profiles, inspect instance logs, transfer files (`push`/`pull`), and manage locally cached base images.

## Prerequisites & Installation

IncusCubby is currently built from source on Linux (no prebuilt release binaries are published yet).

- **Incus daemon** running with `/var/lib/incus/unix.socket` accessible to your user:
  ```bash
  sudo usermod -aG incus-admin "$USER"
  # Log out and back in for the group membership to take effect
  ```
- **Rust 1.85+** (`edition = "2024"`).
- **`incus` CLI** in `PATH` (only required for the *Shell* button, which spawns `incus exec <name> -- su -l root`).

```bash
git clone https://github.com/nandazahran/IncusCubby.git
cd IncusCubby
cargo run --release
```

Build instructions, repository layout, and test commands are covered in [CONTRIBUTING.md](CONTRIBUTING.md).

## Usage

1. Ensure `incusd` is running and your user belongs to `incus-admin`.
2. Launch `cargo run --release` (or the compiled `target/release/incus-cubby` binary).
3. Use the top-level tabs:
   - **Sandboxes**: Select an instance to inspect telemetry, start/stop/restart/freeze, open a shell or IDE, clone, edit CPU/RAM limits, upgrade/install packages, manage disk mounts and proxy ports, take/restore snapshots, view logs, or push/pull files.
   - **Create Sandbox**: Pick `container` or `virtual-machine`, choose a preset or fetched `images.linuxcontainers.org` alias, set CPU/memory/profiles, optionally bind-mount a host directory at `/workspace`, and optionally supply `cloud-init.user-data`.
   - **Storage & Networks**: View pool capacity, create/delete custom volumes, create/delete managed bridge networks (with IPv4/IPv6 NAT toggle), and inspect DHCP leases.
   - **Profiles**: Create, update, or delete non-`default` Incus profiles with CPU and memory limits.
   - **Images**: Inspect and delete locally cached Incus base images.
   - **Settings**: Persist preferred terminal emulator and IDE to `$XDG_CONFIG_HOME/incus-cubby/settings.json` (or `~/.config/incus-cubby/settings.json`).

## Documentation

- [Architecture Specification](docs/ARCHITECTURE.md)
- [Incus REST API & Simplestreams Reference](docs/INCUS_API.md)
- [Roadmap & Risk Matrix](docs/ROADMAP.md)
- [Privacy Policy](docs/PRIVACY.md) — local-first, zero telemetry.

## Prior Art & References

IncusCubby is an independent Rust + Slint implementation under the GNU General Public License v3.0 or later (`LICENSE`). Inspiration and references consulted:

- [Incus](https://linuxcontainers.org/incus/) — System container and virtual machine manager (`1.0` Unix socket REST API & Web UI workflows).
- [DistroShelf](https://github.com/ranfdev/DistroShelf) & [BoxBuddyRS](https://github.com/Dvlv/BoxBuddyRS) — Linux desktop container manager UX workflows (clone, package upgrade/install, terminal/IDE integration).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for building from source, the repository layout, live smoke testing, and the checks enforced locally.

## License

Licensed under the [GNU General Public License v3.0 or later (`GPL-3.0-or-later`)](LICENSE).
