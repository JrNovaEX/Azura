//! Linux (freedesktop) integration.
//!
//! * Discovery reads `.desktop` entries from the XDG data directories, which
//!   include Flatpak and Snap exports; entries earlier in the search path
//!   override later ones with the same id, as the spec requires.
//! * Icons resolve through the icon theme (current theme, its parents, then
//!   `hicolor`) or `/usr/share/pixmaps`. PNG and SVG are supported.
//! * Apps launch by their parsed `Exec=` line, without a shell. Commands run
//!   in the user's login shell, in a terminal or in the background.

use std::collections::HashMap;
use std::fs;
use std::io::Seek as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use azura_core::{AppTarget, LaunchError};

use crate::desktop_entry::{DesktopEntry, parse_exec};
use crate::unix::{find_in_path, language_from_env, spawn_detached, user_shell};
use crate::{DiscoveredApp, ICON_SIZE, Icon, IconFormat, OpenTarget};

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
}

/// `$XDG_DATA_HOME` followed by `$XDG_DATA_DIRS`, plus the Flatpak and Snap
/// export directories when a distribution forgot to add them.
fn data_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share")),
    ];
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.extend(
        system
            .split(':')
            .filter(|d| !d.is_empty())
            .map(PathBuf::from),
    );
    for extra in [
        home().join(".local/share/flatpak/exports/share"),
        PathBuf::from("/var/lib/flatpak/exports/share"),
        PathBuf::from("/var/lib/snapd/desktop"),
    ] {
        if !dirs.contains(&extra) {
            dirs.push(extra);
        }
    }
    dirs
}

fn current_desktops() -> Vec<String> {
    std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(str::to_owned)
        .collect()
}

pub fn discover_apps() -> Vec<DiscoveredApp> {
    let desktops = current_desktops();
    let locale = language_from_env();
    let mut seen: HashMap<String, ()> = HashMap::new();
    let mut apps = Vec::new();
    for dir in data_dirs() {
        let root = dir.join("applications");
        let mut files = Vec::new();
        collect_desktop_files(&root, &root, &mut files);
        for (id, path) in files {
            // The first directory that defines an id wins, even if hidden.
            if seen.insert(id.clone(), ()).is_some() {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let entry = DesktopEntry::parse(&text);
            if !entry.is_visible_app(&desktops) {
                continue;
            }
            if let Some(try_exec) = entry.get("TryExec")
                && find_in_path(try_exec).is_none()
            {
                continue;
            }
            let Some(name) = entry.localized("Name", locale.as_deref()) else {
                continue;
            };
            apps.push(DiscoveredApp {
                name: name.to_owned(),
                detail: entry
                    .localized("Comment", locale.as_deref())
                    .map(str::to_owned),
                target: AppTarget::DesktopEntry { id, path },
            });
        }
    }
    apps
}

/// Desktop ids are paths relative to `applications/` with `/` turned into `-`.
fn collect_desktop_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_files(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            let id = path
                .strip_prefix(root)
                .map(|rel| rel.to_string_lossy().replace('/', "-"))
                .unwrap_or_default();
            out.push((id, path));
        }
    }
}

fn load_entry(path: &Path) -> Result<DesktopEntry, LaunchError> {
    fs::read_to_string(path)
        .map(|text| DesktopEntry::parse(&text))
        .map_err(|_| LaunchError::NotFound(path.display().to_string()))
}

pub fn check_app(target: &AppTarget) -> Result<(), LaunchError> {
    match target {
        AppTarget::DesktopEntry { path, .. } | AppTarget::Executable { path } if path.exists() => {
            Ok(())
        }
        AppTarget::DesktopEntry { path, .. } | AppTarget::Executable { path } => {
            Err(LaunchError::NotFound(path.display().to_string()))
        }
        // From a session file written on another operating system. Say so
        // plainly rather than claiming the app is missing.
        _ => Err(LaunchError::Unsupported),
    }
}

pub fn target_for_path(path: &Path) -> AppTarget {
    if path.extension().is_some_and(|e| e == "desktop") {
        AppTarget::DesktopEntry {
            id: crate::unix::file_name_lossy(path),
            path: path.to_owned(),
        }
    } else {
        AppTarget::Executable {
            path: path.to_owned(),
        }
    }
}

// ------------------------------------------------------------------- icons

pub fn app_icon(target: &AppTarget) -> Option<Icon> {
    match target {
        AppTarget::DesktopEntry { path, .. } => {
            let entry = load_entry(path).ok()?;
            let icon = entry.get("Icon")?;
            load_icon_by_name(icon)
        }
        // A hand-picked executable has no `.desktop` file of its own. Prefer
        // the icon a desktop entry declares for the same program, since that
        // is the one the system shows elsewhere, and fall back to an image
        // embedded in the binary.
        AppTarget::Executable { path } => match desktop_entry_icon_for(path)
            .as_deref()
            .and_then(load_icon_by_name)
        {
            Some(icon) => Some(icon),
            None => icon_from_binary(path),
        },
        _ => None,
    }
}

/// Resolves an `Icon=` value — an absolute path or a theme icon name — to an
/// image, encoded and cached on disk.
fn load_icon_by_name(icon: &str) -> Option<Icon> {
    let file = if icon.starts_with('/') {
        let path = PathBuf::from(icon);
        path.is_file().then_some(path)
    } else {
        lookup_icon(icon)
    }?;
    let bytes = fs::read(&file).ok()?;
    match file.extension().and_then(|e| e.to_str()) {
        Some("svg") => Some(Icon {
            format: IconFormat::Svg,
            bytes,
        }),
        Some("png") => {
            let image =
                image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
            crate::icon_cache::encode_png(
                image.width(),
                image.height(),
                image.to_rgba8().into_raw(),
            )
        }
        _ => None,
    }
}

/// Finds a `.desktop` entry whose `Exec=` runs `path`, and returns its icon.
fn desktop_entry_icon_for(path: &Path) -> Option<String> {
    let wanted = path.file_name()?;
    let name = wanted.to_string_lossy();
    for dir in data_dirs() {
        let root = dir.join("applications");
        let mut files = Vec::new();
        collect_desktop_files(&root, &root, &mut files);
        for (_, file) in files {
            let Ok(entry) = fs::read_to_string(&file).map(|text| DesktopEntry::parse(&text)) else {
                continue;
            };
            let launches_it = entry
                .get("Exec")
                .and_then(|exec| parse_exec(exec, None, "", "").into_iter().next())
                .is_some_and(|program| {
                    program == name || Path::new(&program).file_name() == Some(wanted)
                });
            if launches_it && let Some(icon) = entry.get("Icon") {
                return Some(icon.to_owned());
            }
        }
    }
    None
}

/// Every PNG starts with these eight bytes.
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

/// Pulls an image out of an executable that carries one.
///
/// Plenty of Linux programs embed their own icon in the binary, because a
/// panel or dock wants one before it has read any `.desktop` file. This looks
/// for a complete PNG in the tail of the file, where linkers and resource
/// sections put them.
fn icon_from_binary(path: &Path) -> Option<Icon> {
    let len = fs::metadata(path).ok()?.len();
    // Icons are small; reading all of a 200 MB game to find one is not worth
    // it, and the resource sections sit at the end.
    const TAIL: u64 = 4 * 1024 * 1024;
    let mut file = fs::File::open(path).ok()?;
    file.seek(std::io::SeekFrom::Start(len.saturating_sub(TAIL)))
        .ok()?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut buf).ok()?;

    let bytes = find_png(&buf)?;
    // Re-encode rather than trust a file we found by scanning: it bounds the
    // size and gives the icon cache a uniform 64px image.
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()?;
    crate::icon_cache::encode_png(image.width(), image.height(), image.to_rgba8().into_raw())
}

/// The bytes of the first well-formed PNG in `buf`.
fn find_png(buf: &[u8]) -> Option<Vec<u8>> {
    let mut from = 0;
    while from + PNG_MAGIC.len() <= buf.len() {
        let at = from
            + buf[from..]
                .windows(PNG_MAGIC.len())
                .position(|window| window == PNG_MAGIC)?;
        if let Some(bytes) = slice_png_at(&buf[at..]) {
            return Some(bytes);
        }
        from = at + 1;
    }
    None
}

/// The bytes of the PNG starting at `start`, found by walking its chunk
/// list. `None` for a truncated or malformed one, which is the common case
/// when the magic number appears inside some other data.
fn slice_png_at(start: &[u8]) -> Option<Vec<u8>> {
    if start.len() < PNG_MAGIC.len() || &start[..PNG_MAGIC.len()] != PNG_MAGIC {
        return None;
    }
    let mut at = PNG_MAGIC.len();
    loop {
        let header = start.get(at..at + 8)?;
        let length = usize::try_from(u32::from_be_bytes(header[..4].try_into().ok()?)).ok()?;
        let is_last = &header[4..8] == b"IEND";
        // Each chunk is a 4-byte length, 4-byte type, the data and a CRC.
        at = at.checked_add(12 + length)?;
        if at > start.len() {
            return None;
        }
        if is_last {
            return Some(start[..at].to_vec());
        }
    }
}

fn icon_base_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![home().join(".icons")];
    dirs.extend(data_dirs().into_iter().map(|d| d.join("icons")));
    dirs
}

/// The icon theme the desktop is using, from GTK or KDE settings.
///
/// Both files are INI-style, but an INI section header is irrelevant here and
/// a comment starts either with `;` or `#`, so a line like `gtk-icon-theme-name=`
/// inside a commented-out block is not mistaken for the real setting.
fn current_theme() -> Option<String> {
    let config = dirs::config_dir()?;
    let read_key = |file: PathBuf, key: &str| -> Option<String> {
        fs::read_to_string(file).ok()?.lines().find_map(|line| {
            let line = line.trim();
            if line.starts_with([';', '#']) {
                return None;
            }
            let (k, v) = line.split_once('=')?;
            let v = v.split('#').next()?.trim().trim_matches('"');
            (k.trim() == key && !v.is_empty()).then(|| v.to_owned())
        })
    };
    read_key(config.join("gtk-4.0/settings.ini"), "gtk-icon-theme-name")
        .or_else(|| read_key(config.join("gtk-3.0/settings.ini"), "gtk-icon-theme-name"))
        .or_else(|| read_key(config.join("kdeglobals"), "Theme"))
        .filter(|t| !t.is_empty())
}

fn theme_parents(theme: &str, bases: &[PathBuf]) -> Vec<String> {
    bases
        .iter()
        .find_map(|base| fs::read_to_string(base.join(theme).join("index.theme")).ok())
        .and_then(|text| {
            text.lines().find_map(|line| {
                line.strip_prefix("Inherits=")
                    .map(|v| v.split(',').map(|s| s.trim().to_owned()).collect())
            })
        })
        .unwrap_or_default()
}

fn lookup_icon(name: &str) -> Option<PathBuf> {
    let bases = icon_base_dirs();
    let mut themes: Vec<String> = Vec::new();
    if let Some(theme) = current_theme() {
        themes.extend(theme_parents(&theme, &bases));
        themes.insert(0, theme);
    }
    for fallback in ["hicolor", "Adwaita", "breeze"] {
        if !themes.iter().any(|t| t == fallback) {
            themes.push(fallback.to_owned());
        }
    }
    let wanted = ICON_SIZE;
    let sizes = [
        format!("{wanted}x{wanted}"),
        "48x48".to_owned(),
        "96x96".to_owned(),
        "128x128".to_owned(),
        "256x256".to_owned(),
        "scalable".to_owned(),
        "32x32".to_owned(),
    ];
    for theme in &themes {
        for base in &bases {
            let root = base.join(theme);
            if !root.is_dir() {
                continue;
            }
            for size in &sizes {
                // Both "48x48/apps" (freedesktop) and "apps/48" (some themes).
                for dir in [
                    root.join(size).join("apps"),
                    root.join("apps")
                        .join(size.split('x').next().unwrap_or(size)),
                ] {
                    for ext in ["png", "svg"] {
                        let candidate = dir.join(format!("{name}.{ext}"));
                        if candidate.is_file() {
                            return Some(candidate);
                        }
                    }
                }
            }
        }
    }
    ["png", "svg"].iter().find_map(|ext| {
        let candidate = PathBuf::from(format!("/usr/share/pixmaps/{name}.{ext}"));
        candidate.is_file().then_some(candidate)
    })
}

// ---------------------------------------------------------------- launching

pub fn launch_app(target: &AppTarget) -> Result<(), LaunchError> {
    match target {
        AppTarget::DesktopEntry { path, .. } => {
            let entry = load_entry(path)?;
            let name = entry.get("Name").unwrap_or_default().to_owned();
            let argv = parse_exec(
                entry.get("Exec").unwrap_or_default(),
                entry.get("Icon"),
                &name,
                &path.to_string_lossy(),
            );
            let Some((program, args)) = argv.split_first() else {
                return Err(LaunchError::NotFound(path.display().to_string()));
            };
            if entry.terminal() {
                let mut line = crate::unix::sh_quote(program);
                for arg in args {
                    line.push(' ');
                    line.push_str(&crate::unix::sh_quote(arg));
                }
                let cwd = entry.get("Path").map(Path::new);
                return run_command(&line, cwd, true, None);
            }
            let mut cmd = Command::new(program);
            cmd.args(args);
            match entry.get("Path").filter(|p| !p.is_empty()) {
                Some(dir) => cmd.current_dir(dir),
                None => cmd.current_dir(home()),
            };
            spawn_detached(&mut cmd, program)
        }
        AppTarget::Executable { path } => {
            let mut cmd = Command::new(path);
            if let Some(dir) = path.parent() {
                cmd.current_dir(dir);
            }
            spawn_detached(&mut cmd, &path.display().to_string())
        }
        _ => Err(LaunchError::Unsupported),
    }
}

pub fn open(target: &OpenTarget) -> Result<(), LaunchError> {
    let arg = match target {
        OpenTarget::Url(url) => url.clone(),
        OpenTarget::Path(path) => path.display().to_string(),
    };
    // xdg-open can block until the opened program exits on some desktops,
    // so it is detached rather than awaited.
    spawn_detached(Command::new("xdg-open").arg(arg), "xdg-open")
}

/// Terminal emulators and the flags that make them run a command.
const TERMINALS: &[(&str, &[&str])] = &[
    ("xdg-terminal-exec", &[]),
    ("x-terminal-emulator", &["-e"]),
    ("kgx", &["--"]),
    ("gnome-terminal", &["--"]),
    ("ptyxis", &["--"]),
    ("konsole", &["-e"]),
    ("xfce4-terminal", &["-x"]),
    ("mate-terminal", &["-x"]),
    ("tilix", &["-e"]),
    ("alacritty", &["-e"]),
    ("kitty", &[]),
    ("foot", &[]),
    ("wezterm", &["start", "--"]),
    ("xterm", &["-e"]),
];

fn terminal_prefix(user_choice: Option<&str>) -> Option<Vec<String>> {
    let split = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
    if let Some(choice) = user_choice.filter(|c| !c.trim().is_empty()) {
        return Some(split(choice));
    }
    if let Ok(term) = std::env::var("TERMINAL")
        && !term.is_empty()
        && find_in_path(&term).is_some()
    {
        return Some(vec![term, "-e".to_owned()]);
    }
    TERMINALS.iter().find_map(|(program, flags)| {
        find_in_path(program).map(|_| {
            let mut prefix = vec![(*program).to_owned()];
            prefix.extend(flags.iter().map(|f| (*f).to_owned()));
            prefix
        })
    })
}

pub fn run_command(
    line: &str,
    cwd: Option<&Path>,
    terminal: bool,
    terminal_override: Option<&str>,
) -> Result<(), LaunchError> {
    let shell = user_shell();
    let (program, args) = if terminal {
        let mut argv = terminal_prefix(terminal_override).ok_or(LaunchError::NoTerminal)?;
        // Keep the shell open afterwards so the output stays readable.
        let keep_open = format!("{line}; exec {} -l", crate::unix::sh_quote(&shell));
        argv.extend([shell.clone(), "-l".to_owned(), "-c".to_owned(), keep_open]);
        let program = argv.remove(0);
        (program, argv)
    } else {
        (
            shell.clone(),
            vec!["-l".to_owned(), "-c".to_owned(), line.to_owned()],
        )
    };
    let mut cmd = Command::new(&program);
    cmd.args(&args);
    cmd.current_dir(cwd.map(Path::to_path_buf).unwrap_or_else(home));
    spawn_detached(&mut cmd, &program)
}

/// The terminal emulator Azura would pick on its own, shown as a hint in
/// Settings so the user can see what the empty field means.
pub fn detected_terminal() -> Option<String> {
    let argv = terminal_prefix(None)?;
    let (program, flags) = argv.split_first()?;
    let mut out = program.clone();
    for flag in flags {
        out.push(' ');
        out.push_str(flag);
    }
    Some(out)
}

fn autostart_entry() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("autostart/azura.desktop"))
}

pub fn start_at_login() -> bool {
    autostart_entry()
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|text| DesktopEntry::parse(&text))
        .is_some_and(|entry| {
            entry.get("Hidden") != Some("true")
                && entry.get("X-GNOME-Autostart-enabled") != Some("false")
        })
}

pub fn set_start_at_login(enabled: bool, exe: &Path) -> Result<(), String> {
    let path = autostart_entry().ok_or("no config folder")?;
    if !enabled {
        return match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    let exe = exe.to_string_lossy().replace('"', "\\\"");
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=Azura\nExec=\"{exe}\" {}\nX-GNOME-Autostart-enabled=true\n",
        crate::STARTUP_FLAG
    );
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    fs::write(&path, entry).map_err(|e| e.to_string())
}

pub fn system_language() -> Option<String> {
    language_from_env()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real, decodable PNG so the scanner is tested against real bytes.
    fn tiny_png() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let mut bytes = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn finds_a_png_anywhere_in_a_binary() {
        let png = tiny_png();
        for pad in [0, 7, 512, 4096] {
            let mut buf = vec![0xABu8; pad];
            buf.extend_from_slice(&png);
            buf.extend_from_slice(b"trailing junk");
            assert_eq!(find_png(&buf).as_deref(), Some(&png[..]), "pad {pad}");
        }
    }

    #[test]
    fn stops_at_the_end_of_the_image() {
        let png = tiny_png();
        // Two images back to back: only the first is returned, not both.
        let mut buf = png.clone();
        buf.extend_from_slice(&png);
        assert_eq!(find_png(&buf).unwrap().len(), png.len());
    }

    #[test]
    fn ignores_magic_numbers_that_are_not_a_whole_image() {
        let png = tiny_png();
        // A signature with nothing after it.
        assert!(find_png(PNG_MAGIC).is_none());
        // A signature whose chunk lengths run past the end of the buffer.
        let mut cut = png.clone();
        cut.truncate(png.len() - 4);
        assert!(find_png(&cut).is_none());
        // The magic number at the very end, with no room for a header.
        let mut tail = png[..PNG_MAGIC.len()].to_vec();
        tail.truncate(PNG_MAGIC.len() - 1);
        assert!(find_png(&tail).is_none());
    }

    #[test]
    fn plain_data_has_no_png() {
        assert!(find_png(b"").is_none());
        assert!(find_png(b"\x7fELF\x02\x01\x01\x00 not a png").is_none());
    }

    #[test]
    fn finds_an_icon_embedded_in_an_executable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fake-app");
        let mut bytes = b"\x7fELF\x02\x01\x01\x00".to_vec();
        bytes.extend_from_slice(&tiny_png());
        fs::write(&path, &bytes).unwrap();

        let icon = icon_from_binary(&path).expect("an embedded icon");
        assert_eq!(icon.format, IconFormat::Png);
        let decoded = image::load_from_memory_with_format(&icon.bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        // Bounded by the cache's fixed size; smaller images are left alone.
        assert!(decoded.width() <= ICON_SIZE && decoded.height() <= ICON_SIZE);
        assert_eq!((decoded.width(), decoded.height()), (2, 2));
    }

    #[test]
    fn a_large_embedded_icon_is_scaled_down_to_the_cache_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big-app");
        let big = image::RgbaImage::from_pixel(256, 256, image::Rgba([1, 2, 3, 255]));
        let mut bytes = b"\x7fELF".to_vec();
        let mut png = Vec::new();
        big.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        bytes.extend_from_slice(&png);
        fs::write(&path, &bytes).unwrap();

        let icon = icon_from_binary(&path).expect("an embedded icon");
        let decoded = image::load_from_memory_with_format(&icon.bytes, image::ImageFormat::Png)
            .unwrap()
            .to_rgba8();
        assert_eq!(decoded.dimensions(), (ICON_SIZE, ICON_SIZE));
    }

    #[test]
    fn an_executable_without_an_icon_yields_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain-app");
        fs::write(&path, b"\x7fELF\x02\x01\x01\x00no image here").unwrap();
        assert!(icon_from_binary(&path).is_none());
    }

    #[test]
    fn a_missing_executable_yields_nothing() {
        assert!(icon_from_binary(Path::new("/nonexistent/binary")).is_none());
    }
}
