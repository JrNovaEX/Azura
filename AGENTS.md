# AGENTS.md

Guidance for anyone, human or coding agent, working on Azura. Read this
before changing code. User-facing docs are in [README.md](README.md),
build details in [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md), and
contribution rules in [CONTRIBUTING.md](CONTRIBUTING.md).

## What Azura is

A small, private desktop utility (Rust + GPUI). Users save **sessions**
("Morning", "Deep work"…): ordered lists of real apps, shell commands and
links. Pressing **Run** launches them one by one, with an optional pause after
each item. Azura opens at sign-in (can be turned off) and waits idle, so the
computer boots light and the user's setup starts when they decide.

It is **not** a startup manager, process manager, optimizer, automation
platform or tracker. Reject features that push it in those directions.

## Non-negotiable rules

- **Real data only.** Never add demo entries, a built-in app catalog,
  hardcoded app names, or bundled third-party icons. App names and icons come
  from the OS at runtime. Test fixtures stay inside `#[cfg(test)]`.
- **Private.** No accounts, telemetry or analytics. The update check in
  `azura-platform/src/update.rs` is the **only** network access: GitHub's
  latest-release API plus, when a newer release exists, its Linux archive and
  `SHA256SUMS.txt`. No user data is sent. Do not add other requests.
- **Honest status.** Report only what the OS told us: `Launched`, `Failed`
  (with the OS reason) or `Skipped`. Never claim an app is "running" or
  "ready".
- **Light.** No background services, no filesystem scanning while idle.
  Discovery runs only when the app picker opens; icons load lazily and are
  cached on disk.
- **Never write to a config we could not read.** If `azura.toml`'s *contents*
  are unusable it is moved aside (`Config::back_up_broken`). If it merely could
  not be *read* — wrong permissions, read-only mount — Azura leaves it alone
  and refuses to start; see `ConfigError::file_contents_are_broken`.
- **Commits.** Use only the repository owner's git identity. Do not add
  AI co-author trailers or "generated with" lines to commits or PRs.

## Layout

```text
crates/
  azura-core/        Model, TOML config, launch engine. No OS or UI code.
    model.rs          Session, LaunchItem, ItemKind, AppTarget
    config.rs         load/save (atomic), schema, normalization
    engine.rs         sequential runner, RunEvent, CancelToken, Launcher trait
    version.rs        release versions and their ordering
  azura-platform/    OS integration behind one facade (lib.rs).
    linux.rs          XDG .desktop discovery, icon themes, Exec launch,
                      terminal detection, XDG autostart
    single_instance.rs  flock on $XDG_RUNTIME_DIR/azura.lock, plus the
                      SIGUSR1 "bring me forward" handshake
    desktop_entry.rs  pure .desktop / Exec= parsing
    unix.rs           process helpers for the Linux module
    icon_cache.rs     on-disk PNG/SVG icon cache
    open_target.rs    URL/path classification, allowed URL schemes
    update.rs         GitHub release check and verified download. It never
                      installs anything; that is left to the user
  azura/             The GPUI app.
    main.rs           args (--run, --startup, --updated, --version), window setup
    app.rs            all state and behavior (Azura struct)
    view.rs           root view, title bar, overlays host
    screens/          welcome, sessions list, session, settings, updates
    overlays/         add picker, item editor, dialogs, sheet helpers
    ui.rs             shared widgets (buttons, segmented, tiles, logo)
    theme.rs          palette (light/dark) and component theme sync
    i18n.rs           languages, string lookup, RTL helpers
    locales/          en.toml, fa.toml
    assets/           icons (Lucide subset), fonts (Vazirmatn), logo
    build.rs          logo sizes for the UI
docs/                 DEVELOPMENT.md, media/ (posters and their HTML sources)
packaging/linux/      .desktop file, build-deb.sh, azura.flatpak
```

Keep the dependency direction: `azura` → `azura-platform` → `azura-core`.
OS code belongs only in `azura-platform`. Azura is Linux-only: there is one
platform module, no `#[cfg]` selection, and no cross-platform branches to
carry.

## Commands

```sh
cargo build                     # debug build
cargo run                       # run the app
cargo test                      # all unit tests
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo run -p azura-platform --example list_apps [filter]   # see discovery
packaging/linux/build-deb.sh <version> target/release/azura dist   # the .deb
```

Before finishing any change: `fmt`, `clippy -D warnings` and `test` must pass.
Linux is the only target, so CI covers everything; there is no
cross-compilation to arrange.

## Testing the UI safely

- Set `AZURA_CONFIG=<path>` to use a throwaway sessions file. Never test
  against the user's real `azura.toml`.
- Only one instance runs at a time (an advisory `flock`); starting Azura
  again brings the existing window forward.
- Finishing the welcome screen enables start-at-login
  (`~/.config/autostart/azura.desktop`). Remove that entry after testing with
  a debug build.
- Test updates against a local fake release: serve a `release.json` shaped
  like GitHub's API, with a `azura-<version>-linux-x64.tar.gz` asset and a
  matching `SHA256SUMS.txt`, and set
  `AZURA_UPDATE_URL=http://127.0.0.1:<port>/release.json`. Downloads land in
  `~/.cache/Azura/updates`.
- The daily check records `last_update_check` in the config; remove it to
  check again right away.
- Running a session really launches apps. Prefer harmless targets
  (`echo`, `xeyes`, a text editor) and close what you opened.

## Text and translations

- Every user-facing string lives in `crates/azura/locales/*.toml`; add new
  keys to **every** language. Tests check missing keys and lost
  `{placeholders}`.
- Persian follows [TYPOGRAPHY.md](TYPOGRAPHY.md): casual tone («رو»),
  no tanween, no trailing periods, no `!`, real ZWNJ half-spaces, Persian
  digits, «،» instead of «·», «روتین» for session. A test enforces the
  mechanical rules.
- GPUI cannot wrap right-to-left paragraphs: break long Persian strings with
  `\n` (about 40 characters per line). `I18n::directional` wraps each line
  in an RTL embedding; use `I18n::join` to combine fragments.
- Build layouts with `ui::hrow` / `ui::vstack` so they mirror in RTL.

## Design rules

- Palette from the logo: charcoal tile, cream bowl, one amber light.
- Primary actions are solid (`p.primary`: charcoal on light, cream on dark).
- Amber (`p.accent`) only means "on": running item, active wait, enabled
  switch, focused field.
- No gradients, glows or decorative tints; depth from borders and surface
  steps. Lucide icons at 1.75 stroke.
- GPUI panics if `.hover()` is set twice on one element; helpers like
  `ui::button` already set it.

## Releases

Manual only: Actions → **Release** → enter a version. It bumps `Cargo.toml`,
tags, builds the Linux `.deb`, the Flatpak bundle and
`azura-<version>-linux-x64.tar.gz`, and creates a **draft** release for
review.

Two file names have to stay in step:

- `is_archive` in `azura-platform/src/update.rs` matches what the updater
  downloads.
- the tarball step in `.github/workflows/release.yml` produces it.

`SHA256SUMS.txt` is generated at release time and verified by the updater
before a download is kept.
