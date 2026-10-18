# Developing Azura

This page is for people who want to build Azura, understand how it works, or
contribute. If you just want to use it, the [README](../README.md) is all you
need. To contribute, also read [CONTRIBUTING.md](../CONTRIBUTING.md).

## Install and build

Azura is written in Rust with [GPUI](https://www.gpui.rs/) (through
[GPUI Kit](https://gpui-kit.com)).

```sh
cargo build --release
# The binary is target/release/azura
```

Requirements: a recent stable Rust (developed with 1.97).
Also install GPUI's system libraries, for example on Debian/Ubuntu:

```sh
sudo apt install libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
  libvulkan-dev libx11-xcb-dev libxcb1-dev libfontconfig-dev libfreetype-dev \
  libgl1-mesa-dev libxcb-shape0-dev libxcb-randr0-dev libxcb-xfixes0-dev \
  libxcb-cursor-dev libssl-dev
```

## Using Azura

1. Create a session and give it a name and an icon.
2. **Add** items:
   - **Apps:** search the apps installed on this computer. Press Enter to add
     the best match, or use "Browse for a program…" for anything not listed.
   - **Command:** any command line, for example `npm run dev`. It runs with
     your login shell in a new terminal window, or silently in the background
     if you turn the terminal off. *Settings → Terminal* chooses the emulator;
     leave it empty and Azura finds one.
   - **Link / File:** a web link (`github.com` works), a file or a folder,
     opened with its default app.
3. Click an item to rename it, edit it, change its wait time, or move it.
   Drag items to reorder them.
4. Press **Run**. You can skip a wait or stop the session at any time.

### Command line

```text
azura                 Open Azura
azura --run <name>    Open Azura and run the session called <name>
azura --version       Print the version
```

`--run` works well in a desktop shortcut, e.g. `azura --run Morning`.
Combined with *Settings → Close after a successful run*, the shortcut starts
your routine and gets out of the way. Only one Azura runs at a time. If it is
already open, starting it again brings the existing window forward (and
`--run` is ignored in that case).

### Start at sign-in

Finishing the first-run screen adds Azura to the programs that start when
you sign in. *Settings → Open when you sign in* turns it on or off:

It lives in `~/.config/autostart/azura.desktop`. The entry starts
`azura --startup`, and Azura keeps it pointing at the current executable if
you move or update the app.

Turning it off elsewhere is respected: Azura reads that file back, so
disabling the entry in your desktop's startup settings turns it off in Azura
too.

### The sessions file

Everything is stored in one TOML file:

It is `~/.config/Azura/azura.toml`.

Set `AZURA_CONFIG` to use another file, for example for a portable setup.
*Settings → Sessions file* reveals it. The file is safe to edit while Azura is
closed:

```toml
schema = 1

[settings]
language = "en"
theme = "system"          # system | light | dark
close_after_run = false

[[session]]
name = "Morning"
badge = "sunrise"

[[session.item]]
name = "My VPN"
kind = "app"
wait_after_secs = 5
target = { type = "desktop_entry", id = "my-vpn", path = "/usr/share/applications/my-vpn.desktop" }

[[session.item]]
name = "dev server"
kind = "command"
line = "npm run dev"
cwd = "/home/you/site"     # optional
terminal = true           # false = run in the background without a window

[[session.item]]
name = "Docs"
kind = "open"
target = "https://example.com"
```

If the file's **contents** are unusable, Azura moves it aside
(`azura.toml.broken-<time>`), says so, and starts with an empty list. If the
file merely could not be **read** (a read-only mount, wrong permissions), Azura
leaves it exactly where it is and refuses to start: continuing would mean
saving an empty file over sessions you can still recover by hand.

A session file written on Windows or macOS still loads; those items are marked
as unsupported rather than silently dropped, so your list survives.

## Platform support

Azura is Linux-only. It builds and runs on x86_64 with X11 or Wayland.

| Capability             | How |
| ---------------------- | --- |
| App discovery          | XDG `.desktop` entries in `$XDG_DATA_DIRS`, including the Flatpak and Snap export directories |
| Real icons             | The `Icon=` key through the current icon theme (GTK or KDE settings, its parents, then `hicolor`); PNG and SVG |
| Icons for hand-picked executables | The `Icon=` of a `.desktop` file that runs the same program, then a PNG embedded in the binary itself |
| Launch apps            | The parsed `Exec=` line, run directly with no shell |
| Commands in a terminal | `$TERMINAL`, then `xdg-terminal-exec`, then a list of known emulators; overridable in Settings |
| Open links and files   | `xdg-open` |
| Only one instance      | An advisory `flock` on `$XDG_RUNTIME_DIR/azura.lock` |

Discovery only runs when you open the app picker, and icons are extracted
lazily for visible rows and cached on disk. Azura has no background service.
When idle it uses no CPU.

## Privacy and security

- Azura collects nothing. Its only network access is the update check:
  one request to GitHub's latest-release API (at most daily, can be turned
  off) and, when a newer release exists, downloading its Linux archive and
  `SHA256SUMS.txt`. The download is verified against the published checksum
  before it is kept. Installing it is left to you.
- Nothing runs unless you press **Run** (or start Azura with `--run`).
- Apps are launched directly, without a shell. Only items you created as a
  **Command** go through a shell, exactly as you typed them, and the UI says
  so.
- Links are limited to `http`, `https`, `mailto`, `ftp` and `file`. Other
  schemes can trigger arbitrary protocol handlers, so they are refused.
- Azura never asks for administrator rights. An app that needs them shows its
  own prompt.
- The sessions file can contain commands, so treat a file someone sends you
  like a script: read it before using it. It is created readable only by you.

## Project layout

```text
crates/
  azura-core/       model, TOML config, launch engine. No OS or UI code.
  azura-platform/   discovery, icons, launching, the single-instance lock
  azura/            the GPUI app: screens, overlays, theme, translations
packaging/linux/     the .deb script and the Flatpak manifest
```

The dependency direction is `azura` → `azura-platform` → `azura-core`.
Nothing in `azura-core` touches the operating system.

### Adding a language

1. Copy `crates/azura/locales/en.toml` to `<code>.toml` and translate the
   values. Keep the `{placeholders}`.
2. Add an entry to `LANGUAGES` in `crates/azura/src/i18n.rs` (native name,
   right-to-left or not, native digits if any).
3. Run `cargo test`. It checks that every key exists and that no placeholder
   was lost.

For right-to-left languages, put explicit `\n` breaks in long strings. The
text engine does not wrap right-to-left paragraphs correctly yet, so each
line should fit the compact window (about 40 characters).

## Design

The look comes from the logo: a charcoal tile, the cream bowl of «ن» (which
also reads as a horizon) and one small amber light rising out of it.

- Primary actions are solid: charcoal on the light theme, cream on the dark
  theme. There are no gradients, glows or decorative tints.
- Amber means "on" and nothing else: a running item, an active wait, an
  enabled switch, the focused field.
- Depth comes from borders and surface steps rather than shadows. The UI
  icons are [Lucide](https://lucide.dev) at a 1.75 px stroke.

The logo is rendered in Blender from `crates/azura/assets/logo/azura-logo.blend`
(scene `AzuraLogo`). The build script turns `azura.png` into the in-app sizes,
and `packaging/linux/build-deb.sh` downsizes it for the icon theme, so
re-render the PNG after changing the model.

### Persian copy

All Persian text follows [`TYPOGRAPHY.md`](../TYPOGRAPHY.md): a casual, modern
tone («رو», «می‌تونی»), no tanween, no trailing periods or exclamation marks,
real half-spaces (ZWNJ) and Persian digits. Fragments are joined with «،»,
because a middle dot looks like the Persian zero (۰). `cargo test` checks
these rules on every string in `locales/fa.toml`.

## Development

```sh
cargo test                                   # all unit tests
cargo run -p azura-platform --example list_apps [filter]   # what discovery sees
cargo clippy --all-targets
```

## Updates

`crates/azura-platform/src/update.rs` asks
`https://api.github.com/repos/JrNovaEX/Azura/releases/latest` for the newest
published release (drafts and pre-releases are never offered), at most once a
day and only when **Settings → Check for updates automatically** is on.

When a newer release exists, its `azura-<version>-linux-x64.tar.gz` is
downloaded into `~/.cache/Azura/updates` and checked against the
`SHA256SUMS.txt` entry for that exact file name. On a mismatch the download is
deleted and nothing is kept. Azura then shows you where the file is and stops
there: installing is yours to do, because a `.deb`, a Flatpak and a
hand-unpacked tarball are three different jobs.

`--updated` is the flag a user can pass after reinstalling by hand; Azura
shows a short "up to date" notice and nothing else.

- `HTTPS_PROXY` / `ALL_PROXY` are respected. `AZURA_UPDATE_URL` points the
  check at another server for testing.

The Release workflow publishes exactly the file names the updater looks for, so
keep `is_archive` in `update.rs` and the tarball step in `release.yml` in sync.

## Packaging

`packaging/linux/build-deb.sh` builds `azura_<version>_amd64.deb` from an
already-compiled binary:

```sh
cargo build --release
packaging/linux/build-deb.sh 1.2.3 target/release/azura dist
```

It lays out the standard Debian tree, writes `DEBIAN/control`, and assembles
the `ar` archive with `dpkg-deb` when that is available and by hand when it is
not. Uninstalling leaves `~/.config/Azura` alone: the sessions are the user's,
not ours.

`packaging/linux/azura.flatpak` is a [flatpak-builder](https://docs.flatpak.org)
manifest. It needs the freedesktop 24.08 SDK:

```sh
flatpak-builder --user --force-clean --repo=repo build packaging/linux/azura.flatpak
flatpak build-bundle repo dist/azura.flatpak io.github.jrnovaex.azura stable
```

> The Flatpak is a sandbox, so Azura inside it cannot see the whole home
> directory and some sessions will not find the apps they name. The `.deb` is
> the better choice.

## Releasing

Releases are made by hand with the **Release** workflow:

1. Open the repository's **Actions** tab, choose **Release**, then
   **Run workflow** on the `main` branch.
2. Enter the new version without the `v`, for example `0.2.3`.
3. The workflow checks the version, sets it in `Cargo.toml`, commits
   "Release v0.2.3", tags it `v0.2.3`, then builds the Linux release: the
   `.deb`, the Flatpak bundle, and the `azura-0.2.3-linux-x64.tar.gz` that
   the in-app updater downloads. Tests run first.
4. It then creates a **draft** release with those files, a `SHA256SUMS.txt`
   and generated notes. Review it on the Releases page and publish it.

## Licenses of bundled material

Azura is MIT licensed (see [LICENSE](../LICENSE)). It bundles icons from
[Lucide](https://lucide.dev) (ISC, `crates/azura/assets/icons/LICENSE-LUCIDE`)
and the [Vazirmatn](https://github.com/rastikerdar/vazirmatn) typeface (SIL OFL
1.1, `crates/azura/assets/fonts/OFL.txt`).
