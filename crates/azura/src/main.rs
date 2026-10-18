mod app;
mod assets;
mod i18n;
mod overlays;
mod screens;
mod theme;
mod ui;
mod view;

use azura_core::Config;
use gpui_kit::{AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size};

use crate::app::Azura;

const USAGE: &str = "\
Azura: launch your apps and commands together, in order.

Usage:
  azura                 Open Azura
  azura --run <name>    Open Azura and run the session called <name>
  azura --version       Print the version

Azura passes --startup when the desktop starts it, and --updated when it has
just been reinstalled. Both are for internal use.

Set AZURA_CONFIG to use a different sessions file.";

/// What `--run` asked for, once the arguments have been read.
struct Args {
    run_on_start: Option<String>,
    just_updated: bool,
}

fn parse_args() -> Args {
    let mut parsed = Args {
        run_on_start: None,
        just_updated: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--run" | "-r" => parsed.run_on_start = args.next(),
            // Passed by the sign-in entry; Azura simply opens.
            flag if flag == azura_platform::STARTUP_FLAG => {}
            // Passed after the user reinstalled Azura by hand.
            flag if flag == azura_platform::update::UPDATED_FLAG => parsed.just_updated = true,
            "--version" | "-V" => {
                println!("azura {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("azura: unknown argument {other:?}\n");
                eprint!("{USAGE}");
                std::process::exit(2);
            }
        }
    }
    parsed
}

/// Reads the sessions file, keeping the user's data out of harm's way.
///
/// A file that cannot be parsed is moved aside and the app starts empty, with
/// the backup path shown in a notice. A file that merely could not be *read*
/// is left exactly where it is and Azura refuses to start: carrying on would
/// mean saving an empty configuration over sessions the user can still
/// recover by hand.
fn load_config() -> (Config, Option<std::path::PathBuf>) {
    let path = azura_platform::config_path();
    match Config::load(&path) {
        Ok(config) => (config, None),
        Err(err) if !err.file_contents_are_broken() => {
            eprintln!("azura: cannot read {}: {err}", path.display());
            std::process::exit(1);
        }
        Err(err) => {
            eprintln!("azura: {err}");
            (Config::default(), Config::back_up_broken(&path).ok())
        }
    }
}

fn main() {
    let Args {
        run_on_start,
        just_updated,
    } = parse_args();

    let claimed = if just_updated {
        // The previous copy is still closing; give it a moment.
        azura_platform::claim_single_instance_patiently(std::time::Duration::from_secs(15))
    } else {
        azura_platform::claim_single_instance(true)
    };
    if !claimed {
        // Azura is already open; its window was brought to the front.
        return;
    }
    // Must come before any thread exists, so the handler is installed while
    // the process is still single-threaded.
    azura_platform::listen_for_focus_requests();
    azura_platform::update::clean_up();

    // Keep an enabled sign-in entry pointing at this copy of Azura.
    azura_platform::refresh_start_at_login();

    let config_path = azura_platform::config_path();
    let (config, broken_backup) = load_config();

    gpui_kit::application()
        .with_assets(assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            assets::load_fonts(cx);
            cx.set_global(i18n::I18n::new("en"));
            cx.set_global(theme::Palette::light());
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();

            let bounds = Bounds::centered(None, size(px(380.), px(580.)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Azura".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.), px(15.))),
                }),
                app_owns_titlebar_drag: true,
                window_min_size: Some(size(px(340.), px(440.))),
                app_id: Some("azura".into()),
                ..Default::default()
            };
            match gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| {
                    let mut azura = Azura::new(config, config_path, broken_backup, window, cx);
                    if just_updated {
                        azura.notice = Some(i18n::t(cx, "update.reinstalled"));
                    }
                    if let Some(name) = run_on_start {
                        azura.run_by_name(&name, window, cx);
                    }
                    azura
                })
            }) {
                Ok(_) => {}
                Err(err) => {
                    // A window is the whole app. Without one there is nothing
                    // left to show the error in, so report it and stop.
                    eprintln!("azura: cannot open the window: {err}");
                    std::process::exit(1);
                }
            }
            cx.activate(true);
        });
}
