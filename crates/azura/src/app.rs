//! Application state and behaviour. Rendering lives in `screens/` and
//! `overlays/`; this file owns the data and everything that changes it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use azura_core::Version;
use azura_core::engine::{self, ItemStatus};
use azura_core::{
    AppTarget, CancelToken, Config, ItemKind, LaunchItem, MAX_WAIT_SECS, RunEvent, RunSummary,
    Session, ThemePreference,
};
use azura_platform::update::Release;
use azura_platform::{DiscoveredApp, IconFormat, SystemLauncher};
use futures::StreamExt;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, FocusHandle, Focusable, Image, ImageFormat,
    SharedString, Subscription, Task, Window,
};

use crate::i18n::{self, I18n};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Welcome,
    Sessions,
    Session(usize),
    Settings,
}

pub enum Overlay {
    Picker(PickerTab),
    Editor { item: usize },
    ConfirmDelete,
    Badges,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PickerTab {
    Apps,
    Command,
    Open,
}

pub enum AppsState {
    NotLoaded,
    Loading(#[allow(dead_code)] Task<()>),
    Loaded(Arc<Vec<DiscoveredApp>>),
}

pub enum IconSlot {
    Loading,
    Ready(Arc<Image>),
    Missing,
}

/// Where the update check stands. Shown in Settings and, when there is
/// something to act on, as a notice on the home screen.
#[derive(Clone)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    /// Newer release; this system updates by hand from the release page.
    Available(Release),
    Downloading(Version),
    /// Downloaded and verified; installs on restart.
    Ready {
        version: Version,
        path: PathBuf,
    },
    Failed(String),
}

/// Automatic checks run at most this often.
const UPDATE_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the package version is valid")
}

pub struct RunState {
    pub session: usize,
    pub statuses: Vec<ItemStatus>,
    pub waiting: Option<Waiting>,
    pub summary: Option<RunSummary>,
    cancel: CancelToken,
    _tasks: Vec<Task<()>>,
}

#[derive(Clone, Copy)]
pub struct Waiting {
    pub index: usize,
    pub until: Instant,
    pub total: Duration,
}

impl RunState {
    pub fn is_active(&self) -> bool {
        self.summary.is_none()
    }

    pub fn done_count(&self) -> usize {
        self.statuses
            .iter()
            .filter(|s| !matches!(s, ItemStatus::Pending | ItemStatus::Launching))
            .count()
    }
}

/// Text inputs, created once and reused.
pub struct Inputs {
    pub rename: Entity<InputState>,
    pub search: Entity<InputState>,
    pub cmd_line: Entity<InputState>,
    pub cmd_name: Entity<InputState>,
    pub cmd_cwd: Entity<InputState>,
    pub open_target: Entity<InputState>,
    pub open_name: Entity<InputState>,
    pub item_name: Entity<InputState>,
    pub item_line: Entity<InputState>,
    pub item_cwd: Entity<InputState>,
    pub item_target: Entity<InputState>,
    /// The terminal command in Settings.
    pub settings_terminal: Entity<InputState>,
}

pub struct Azura {
    pub config: Config,
    pub config_path: PathBuf,
    pub screen: Screen,
    pub overlay: Option<Overlay>,
    pub run: Option<RunState>,
    pub apps: AppsState,
    pub icons: HashMap<String, IconSlot>,
    pub notice: Option<SharedString>,
    pub renaming: bool,
    pub cmd_terminal: bool,
    /// Mirrors the OS setting; see `azura_platform::start_at_login`.
    pub start_at_login: bool,
    pub update: UpdateState,
    update_task: Option<Task<()>>,
    pub open_error: bool,
    pub inputs: Inputs,
    pub font_family: SharedString,
    /// Lets the focus poller raise this window without borrowing it.
    pub window_handle: AnyWindowHandle,
    pub focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Focusable for Azura {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx))
}

impl Azura {
    pub fn new(
        config: Config,
        config_path: PathBuf,
        broken_backup: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let inputs = Inputs {
            rename: input(window, cx),
            search: input(window, cx),
            cmd_line: input(window, cx),
            cmd_name: input(window, cx),
            cmd_cwd: input(window, cx),
            open_target: input(window, cx),
            open_name: input(window, cx),
            item_name: input(window, cx),
            item_line: input(window, cx),
            item_cwd: input(window, cx),
            item_target: input(window, cx),
            settings_terminal: input(window, cx),
        };
        // The Settings terminal field mirrors the setting, so it is seeded
        // once and then only written back when the user types in it.
        let terminal_entity = inputs.settings_terminal.clone();
        {
            let saved = config.settings.terminal.clone().unwrap_or_default();
            let placeholder = i18n::i18n(cx).t("settings.terminal_placeholder");
            terminal_entity.update(cx, |state, cx| {
                state.set_placeholder(placeholder, window, cx);
                if !saved.is_empty() {
                    state.set_value(saved, window, cx);
                }
            });
        }
        let mut subscriptions = vec![
            cx.subscribe_in(
                &inputs.settings_terminal,
                window,
                |this, _, event, _, cx| {
                    if !matches!(event, InputEvent::Change) {
                        return;
                    }
                    let entity = this.inputs.settings_terminal.clone();
                    let value = entity.read(cx).value();
                    let value = value.trim().to_owned();
                    let current = this.config.settings.terminal.as_deref().unwrap_or_default();
                    if current == value {
                        return;
                    }
                    this.config.settings.terminal = (!value.is_empty()).then_some(value);
                    this.save(cx);
                },
            ),
            cx.subscribe_in(&inputs.rename, window, Self::on_rename_event),
            cx.subscribe_in(
                &inputs.search,
                window,
                |this, _, event, window, cx| match event {
                    InputEvent::Change => cx.notify(),
                    // Enter adds the best match, so keyboard-only adding is quick.
                    InputEvent::PressEnter { .. } => {
                        let best = this
                            .filtered_apps(cx)
                            .first()
                            .and_then(|ix| this.app_at(*ix))
                            .cloned();
                        if let Some(app) = best {
                            this.add_app(app, window, cx);
                        }
                    }
                    _ => {}
                },
            ),
            cx.subscribe_in(&inputs.open_target, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.open_error = false;
                    cx.notify();
                }
            }),
            cx.observe_window_appearance(window, |this, window, cx| {
                if this.config.settings.theme == ThemePreference::System {
                    this.apply_theme(window, cx);
                }
            }),
        ];
        for field in [&inputs.cmd_line, &inputs.cmd_name, &inputs.open_name] {
            subscriptions.push(
                cx.subscribe_in(field, window, |this, _, event, window, cx| {
                    if let InputEvent::PressEnter { .. } = event {
                        this.submit_picker_form(window, cx);
                    }
                }),
            );
        }
        for field in [
            &inputs.item_name,
            &inputs.item_line,
            &inputs.item_cwd,
            &inputs.item_target,
        ] {
            subscriptions.push(
                cx.subscribe_in(field, window, |this, _, event, window, cx| match event {
                    InputEvent::Blur => this.commit_editor_fields(cx),
                    InputEvent::PressEnter { .. } => this.close_editor(window, cx),
                    _ => {}
                }),
            );
        }

        let screen = if config.settings.language.is_none() {
            Screen::Welcome
        } else {
            Screen::Sessions
        };
        let mut this = Self {
            config,
            config_path,
            screen,
            overlay: None,
            run: None,
            apps: AppsState::NotLoaded,
            icons: HashMap::new(),
            notice: None,
            renaming: false,
            cmd_terminal: true,
            start_at_login: azura_platform::start_at_login(),
            update: UpdateState::Idle,
            update_task: None,
            open_error: false,
            inputs,
            font_family: SharedString::default(),
            window_handle: window.window_handle(),
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        };
        this.apply_language(window, cx);
        this.schedule_update_check(cx);
        this.watch_for_focus_requests(cx);
        if let Some(backup) = broken_backup {
            let i = cx.global::<I18n>();
            this.notice = Some(
                format!(
                    "{}
{}",
                    i.t("config_error.title"),
                    i.tf(
                        "config_error.body",
                        &[("path", &backup.display().to_string())]
                    )
                )
                .into(),
            );
        }
        this
    }

    /// Raises the window when another copy of Azura is started.
    ///
    /// The second copy signals us; the handler only flips a flag, because
    /// touching GPUI from a signal handler is not allowed. Polling is the
    /// simplest honest bridge, and half a second is well under the time a
    /// person waits for a window to appear.
    fn watch_for_focus_requests(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(400))
                    .await;
                if azura_platform::take_focus_request() {
                    let _ = this.update(cx, |this, cx| {
                        let _ = this.window_handle.update(cx, |_, window, _| {
                            window.activate_window();
                            window.remove_window();
                        });
                    });
                }
            }
        })
        .detach();
    }

    // ---------------------------------------------------------- persistence

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if let Err(e) = self.config.save(&self.config_path) {
            self.notice = Some(e.to_string().into());
        }
        cx.notify();
    }

    // ------------------------------------------------------- look and feel

    pub fn language_code(&self) -> &'static str {
        i18n::language(
            self.config
                .settings
                .language
                .as_deref()
                .unwrap_or_else(|| i18n::guess_language()),
        )
        .code
    }

    pub fn set_language(&mut self, code: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.config.settings.language = Some(code.to_owned());
        self.apply_language(window, cx);
        self.save(cx);
    }

    pub fn apply_language(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.language_code();
        cx.set_global(I18n::new(code));
        self.font_family = pick_font(code, cx);
        self.apply_theme(window, cx);
        self.refresh_placeholders(window, cx);
    }

    pub fn set_theme(
        &mut self,
        pref: ThemePreference,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.config.settings.theme = pref;
        self.apply_theme(window, cx);
        self.save(cx);
    }

    fn apply_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dark = theme::resolve_dark(self.config.settings.theme, window.appearance());
        theme::apply(dark, self.font_family.clone(), Some(window), cx);
        cx.notify();
    }

    fn refresh_placeholders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let i = cx.global::<I18n>();
        let pairs = [
            (self.inputs.search.clone(), i.t("picker.search")),
            (
                self.inputs.cmd_line.clone(),
                i.t("command.line_placeholder"),
            ),
            (
                self.inputs.cmd_name.clone(),
                i.t("command.name_placeholder"),
            ),
            (self.inputs.cmd_cwd.clone(), i.t("command.cwd_placeholder")),
            (
                self.inputs.open_target.clone(),
                i.t("open.target_placeholder"),
            ),
            (self.inputs.open_name.clone(), i.t("open.name_placeholder")),
            (
                self.inputs.item_line.clone(),
                i.t("command.line_placeholder"),
            ),
            (self.inputs.item_cwd.clone(), i.t("command.cwd_placeholder")),
            (
                self.inputs.item_target.clone(),
                i.t("open.target_placeholder"),
            ),
        ];
        for (input, text) in pairs {
            input.update(cx, |state, cx| state.set_placeholder(text, window, cx));
        }
    }

    // ------------------------------------------------------------ navigation

    pub fn go(&mut self, screen: Screen, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_rename(window, cx);
        self.overlay = None;
        self.screen = screen;
        if let Screen::Session(ix) = screen {
            self.load_item_icons(ix, cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn finish_welcome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let code = self.language_code();
        self.set_language(code, window, cx);
        // Azura starts with the computer by default; Settings can turn it off.
        self.set_start_at_login(true, cx);
        self.go(Screen::Sessions, window, cx);
    }

    // --------------------------------------------------------------- updates

    /// Runs the daily automatic check a little after startup, once no
    /// session is running.
    fn schedule_update_check(&mut self, cx: &mut Context<Self>) {
        let settings = &self.config.settings;
        let due = settings
            .last_update_check
            .is_none_or(|last| now_secs().saturating_sub(last) >= UPDATE_INTERVAL.as_secs());
        if !settings.check_updates || !due {
            return;
        }
        self.update_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(8)).await;
            loop {
                let busy = this
                    .update(cx, |this, _| this.any_running())
                    .unwrap_or(true);
                if !busy {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_secs(30))
                    .await;
            }
            let _ = this.update(cx, |this, cx| this.check_for_updates(cx));
        }));
    }

    pub fn set_check_updates(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.config.settings.check_updates = enabled;
        self.save(cx);
        if enabled {
            self.schedule_update_check(cx);
        }
    }

    pub fn check_for_updates(&mut self, cx: &mut Context<Self>) {
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Downloading(_) | UpdateState::Ready { .. }
        ) {
            return;
        }
        self.update = UpdateState::Checking;
        cx.notify();
        self.update_task = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_executor()
                .spawn(async { azura_platform::update::latest_release() })
                .await;
            let release = match found {
                Ok(release) => release,
                Err(err) => {
                    let _ = this.update(cx, |this, cx| {
                        this.update = UpdateState::Failed(err);
                        cx.notify();
                    });
                    return;
                }
            };
            let newer = release.version > current_version();
            let downloadable = newer && release.downloadable();
            let _ = this.update(cx, |this, cx| {
                this.config.settings.last_update_check = Some(now_secs());
                this.save(cx);
                this.update = match (newer, downloadable) {
                    (false, _) => UpdateState::UpToDate,
                    (true, false) => UpdateState::Available(release.clone()),
                    (true, true) => UpdateState::Downloading(release.version.clone()),
                };
                cx.notify();
            });
            if !downloadable {
                return;
            }
            let downloaded = cx
                .background_executor()
                .spawn({
                    let release = release.clone();
                    async move { azura_platform::update::download(&release) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.update = match downloaded {
                    Ok(path) => UpdateState::Ready {
                        version: release.version.clone(),
                        path,
                    },
                    // Downloading failed: fall back to the manual way.
                    Err(_) => UpdateState::Available(release.clone()),
                };
                cx.notify();
            });
        }));
    }

    /// Opens the downloaded archive's folder, so the user can install it.
    /// Azura never installs anything itself: a `.deb`, a Flatpak and a
    /// hand-copied binary are three different jobs.
    pub fn show_downloaded_update(&mut self, cx: &mut Context<Self>) {
        let UpdateState::Ready { path, .. } = &self.update else {
            return;
        };
        cx.reveal_path(path);
    }

    pub fn open_release_page(&mut self, cx: &mut Context<Self>) {
        let page = match &self.update {
            UpdateState::Available(release) => release.page.clone(),
            _ => azura_platform::update::RELEASES_PAGE.to_owned(),
        };
        cx.open_url(&page);
    }

    pub fn set_start_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if let Err(err) = azura_platform::set_start_at_login(enabled) {
            self.notice = Some(err.into());
        }
        self.start_at_login = azura_platform::start_at_login();
        cx.notify();
    }

    pub fn current_session(&self) -> Option<usize> {
        match self.screen {
            Screen::Session(ix) if ix < self.config.sessions.len() => Some(ix),
            _ => None,
        }
    }

    // -------------------------------------------------------------- sessions

    pub fn create_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let name = i18n::t(cx, "sessions.default_name");
        let mut session = Session::new(name.to_string());
        session.badge = Some(
            crate::assets::BADGES[self.config.sessions.len() % crate::assets::BADGES.len()]
                .to_owned(),
        );
        self.config.sessions.push(session);
        self.save(cx);
        let ix = self.config.sessions.len() - 1;
        self.go(Screen::Session(ix), window, cx);
        self.start_rename(window, cx);
    }

    pub fn start_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        let name = self.config.sessions[ix].name.clone();
        self.renaming = true;
        self.inputs.rename.update(cx, |state, cx| {
            state.set_value(name, window, cx);
            state.select_all(window, cx);
            state.focus(window, cx);
        });
        cx.notify();
    }

    fn on_rename_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
            self.finish_rename(window, cx);
        }
    }

    pub fn finish_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.renaming {
            return;
        }
        self.renaming = false;
        let name = self.inputs.rename.read(cx).value().trim().to_owned();
        if let Some(ix) = self.current_session()
            && !name.is_empty()
            && self.config.sessions[ix].name != name
        {
            self.config.sessions[ix].name = name;
            self.save(cx);
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub fn set_badge(&mut self, badge: &str, cx: &mut Context<Self>) {
        if let Some(ix) = self.current_session() {
            self.config.sessions[ix].badge = Some(badge.to_owned());
            self.overlay = None;
            self.save(cx);
        }
    }

    pub fn delete_current_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        if self.is_running(ix) {
            return;
        }
        self.config.sessions.remove(ix);
        // Keep a finished run pointing at the right session (or drop it).
        if let Some(run) = &mut self.run {
            if run.session == ix {
                self.run = None;
            } else if run.session > ix {
                run.session -= 1;
            }
        }
        self.save(cx);
        self.go(Screen::Sessions, window, cx);
    }

    // ----------------------------------------------------------------- items

    pub fn add_item(&mut self, item: LaunchItem, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        self.clear_finished_run(ix);
        self.config.sessions[ix].items.push(item);
        self.save(cx);
        self.load_item_icons(ix, cx);
        self.overlay = None;
        window.focus(&self.focus, cx);
    }

    pub fn move_item(&mut self, from: usize, to: usize, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        if self.is_running(ix) {
            return;
        }
        self.clear_finished_run(ix);
        self.config.sessions[ix].move_item(from, to);
        if let Some(Overlay::Editor { item }) = &mut self.overlay
            && *item == from
        {
            *item = to;
        }
        self.save(cx);
    }

    pub fn remove_item(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        if self.is_running(ix) || item >= self.config.sessions[ix].items.len() {
            return;
        }
        self.clear_finished_run(ix);
        self.config.sessions[ix].items.remove(item);
        self.overlay = None;
        window.focus(&self.focus, cx);
        self.save(cx);
    }

    pub fn set_wait(&mut self, item: usize, secs: u32, cx: &mut Context<Self>) {
        if let Some(ix) = self.current_session()
            && let Some(it) = self.config.sessions[ix].items.get_mut(item)
        {
            it.wait_after_secs = secs.min(MAX_WAIT_SECS);
            self.save(cx);
        }
    }

    pub fn open_editor(&mut self, item: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.current_session() else {
            return;
        };
        if self.is_running(ix) {
            return;
        }
        let Some(it) = self.config.sessions[ix].items.get(item).cloned() else {
            return;
        };
        let set = |input: &Entity<InputState>, value: String, window: &mut Window, cx: &mut App| {
            input.update(cx, |state, cx| state.set_value(value, window, cx));
        };
        set(&self.inputs.item_name, it.name.clone(), window, cx);
        match &it.kind {
            ItemKind::Command { line, cwd, .. } => {
                set(&self.inputs.item_line, line.clone(), window, cx);
                let cwd = cwd
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                set(&self.inputs.item_cwd, cwd, window, cx);
            }
            ItemKind::Open { target } => set(&self.inputs.item_target, target.clone(), window, cx),
            ItemKind::App { .. } => {}
        }
        self.overlay = Some(Overlay::Editor { item });
        cx.notify();
    }

    /// Writes the editor's text fields back into the item.
    pub fn commit_editor_fields(&mut self, cx: &mut Context<Self>) {
        let (Some(ix), Some(Overlay::Editor { item })) = (self.current_session(), &self.overlay)
        else {
            return;
        };
        let item = *item;
        let value = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_owned();
        let name = value(&self.inputs.item_name, cx);
        let line = value(&self.inputs.item_line, cx);
        let cwd = value(&self.inputs.item_cwd, cx);
        let target = value(&self.inputs.item_target, cx);
        let Some(it) = self.config.sessions[ix].items.get_mut(item) else {
            return;
        };
        let before = it.clone();
        if !name.is_empty() {
            it.name = name;
        }
        match &mut it.kind {
            ItemKind::Command {
                line: l, cwd: c, ..
            } => {
                if !line.is_empty() {
                    *l = line;
                }
                *c = (!cwd.is_empty()).then(|| PathBuf::from(cwd));
            }
            ItemKind::Open { target: t } if !target.is_empty() => *t = target,
            _ => {}
        }
        if *it != before {
            self.save(cx);
        }
    }

    pub fn set_item_terminal(&mut self, item: usize, terminal: bool, cx: &mut Context<Self>) {
        if let Some(ix) = self.current_session()
            && let Some(LaunchItem {
                kind: ItemKind::Command { terminal: t, .. },
                ..
            }) = self.config.sessions[ix].items.get_mut(item)
        {
            *t = terminal;
            self.save(cx);
        }
    }

    pub fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.commit_editor_fields(cx);
        self.overlay = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    // ---------------------------------------------------------------- picker

    pub fn open_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.inputs.search,
            &self.inputs.cmd_line,
            &self.inputs.cmd_name,
            &self.inputs.cmd_cwd,
            &self.inputs.open_target,
            &self.inputs.open_name,
        ] {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.cmd_terminal = true;
        self.open_error = false;
        self.set_picker_tab(PickerTab::Apps, window, cx);
        self.load_apps(false, cx);
    }

    pub fn set_picker_tab(&mut self, tab: PickerTab, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Some(Overlay::Picker(tab));
        let focus = match tab {
            PickerTab::Apps => &self.inputs.search,
            PickerTab::Command => &self.inputs.cmd_line,
            PickerTab::Open => &self.inputs.open_target,
        };
        focus.update(cx, |state, cx| state.focus(window, cx));
        cx.notify();
    }

    pub fn load_apps(&mut self, force: bool, cx: &mut Context<Self>) {
        if !force && !matches!(self.apps, AppsState::NotLoaded) {
            return;
        }
        let task = cx.spawn(async move |this, cx| {
            let apps = cx
                .background_executor()
                .spawn(async move { azura_platform::discover_apps() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.apps = AppsState::Loaded(Arc::new(apps));
                cx.notify();
            });
        });
        self.apps = AppsState::Loading(task);
        cx.notify();
    }

    /// Apps matching the search box, best matches first.
    /// The discovered apps that match the search box, best match first.
    ///
    /// Returns indices into the loaded list rather than copies: the picker
    /// calls this on every render, and cloning every name and path each time
    /// is waste. The indices are boxed so they outlive the borrow.
    pub fn filtered_apps(&self, cx: &App) -> Rc<Vec<usize>> {
        let AppsState::Loaded(apps) = &self.apps else {
            return Rc::new(Vec::new());
        };
        let query = self.inputs.search.read(cx).value().trim().to_lowercase();
        if query.is_empty() {
            return Rc::new((0..apps.len()).collect());
        }
        let mut scored: Vec<(u8, usize)> = apps
            .iter()
            .enumerate()
            .filter_map(|(ix, app)| match_score(&app.name.to_lowercase(), &query).map(|s| (s, ix)))
            .collect();
        scored.sort_by_key(|(score, _)| *score);
        Rc::new(scored.into_iter().map(|(_, ix)| ix).collect())
    }

    /// One app from [`filtered_apps`], for rendering a single row.
    pub fn app_at(&self, index: usize) -> Option<&DiscoveredApp> {
        let AppsState::Loaded(apps) = &self.apps else {
            return None;
        };
        apps.get(index)
    }

    pub fn add_app(&mut self, app: DiscoveredApp, window: &mut Window, cx: &mut Context<Self>) {
        let item = LaunchItem::new(app.name, ItemKind::App { target: app.target });
        self.add_item(item, window, cx);
    }

    /// Adds an app from just its name and target, for the picker's rows.
    /// [`DiscoveredApp::detail`] only ever labels the row, so dropping it here
    /// keeps the click handler from cloning the whole entry on every render.
    pub fn add_app_by_target(
        &mut self,
        name: String,
        target: AppTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.add_item(LaunchItem::new(name, ItemKind::App { target }), window, cx);
    }

    pub fn browse_program(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    let item = LaunchItem::new(
                        azura_platform::name_for_path(&path),
                        ItemKind::App {
                            target: azura_platform::target_for_path(&path),
                        },
                    );
                    this.add_item(item, window, cx);
                });
            }
        })
        .detach();
    }

    /// Picks a file or folder into `target`.
    pub fn browse_into(
        &mut self,
        target: Entity<InputState>,
        directories: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let paths = cx.prompt_for_paths(gpui_kit::PathPromptOptions {
            files: !directories,
            directories,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            if let Ok(Ok(Some(paths))) = paths.await
                && let Some(path) = paths.into_iter().next()
            {
                let _ = this.update_in(cx, |this, window, cx| {
                    target.update(cx, |state, cx| {
                        state.set_value(path.display().to_string(), window, cx)
                    });
                    this.open_error = false;
                    this.commit_editor_fields(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn submit_picker_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let value = |input: &Entity<InputState>, cx: &App| input.read(cx).value().trim().to_owned();
        match self.overlay {
            Some(Overlay::Picker(PickerTab::Command)) => {
                let line = value(&self.inputs.cmd_line, cx);
                if line.is_empty() {
                    return;
                }
                let name = value(&self.inputs.cmd_name, cx);
                let cwd = value(&self.inputs.cmd_cwd, cx);
                let name = if name.is_empty() {
                    command_title(&line)
                } else {
                    name
                };
                let item = LaunchItem::new(
                    name,
                    ItemKind::Command {
                        line,
                        cwd: (!cwd.is_empty()).then(|| PathBuf::from(cwd)),
                        terminal: self.cmd_terminal,
                    },
                );
                self.add_item(item, window, cx);
            }
            Some(Overlay::Picker(PickerTab::Open)) => {
                let target = value(&self.inputs.open_target, cx);
                let classified = azura_platform::open_target::classify(&target);
                let Ok(open) = classified else {
                    self.open_error = true;
                    cx.notify();
                    return;
                };
                let name = value(&self.inputs.open_name, cx);
                let name = if name.is_empty() {
                    open_title(&open)
                } else {
                    name
                };
                self.add_item(LaunchItem::new(name, ItemKind::Open { target }), window, cx);
            }
            _ => {}
        }
    }

    // ----------------------------------------------------------------- icons

    pub fn icon(&mut self, target: &AppTarget, cx: &mut Context<Self>) -> Option<Arc<Image>> {
        let key = target.cache_key();
        match self.icons.get(&key) {
            Some(IconSlot::Ready(image)) => return Some(image.clone()),
            Some(_) => return None,
            None => {}
        }
        self.icons.insert(key.clone(), IconSlot::Loading);
        let target = target.clone();
        cx.spawn(async move |this, cx| {
            let icon = cx
                .background_executor()
                .spawn(async move { azura_platform::app_icon(&target) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let slot = match icon {
                    Some(icon) => {
                        let format = match icon.format {
                            IconFormat::Png => ImageFormat::Png,
                            IconFormat::Svg => ImageFormat::Svg,
                        };
                        IconSlot::Ready(Arc::new(Image::from_bytes(format, icon.bytes)))
                    }
                    None => IconSlot::Missing,
                };
                this.icons.insert(key, slot);
                cx.notify();
            });
        })
        .detach();
        None
    }

    fn load_item_icons(&mut self, ix: usize, cx: &mut Context<Self>) {
        let targets: Vec<AppTarget> = self.config.sessions[ix]
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::App { target } => Some(target.clone()),
                _ => None,
            })
            .collect();
        for target in targets {
            self.icon(&target, cx);
        }
    }

    // ------------------------------------------------------------------- run

    pub fn is_running(&self, session: usize) -> bool {
        self.run
            .as_ref()
            .is_some_and(|r| r.session == session && r.is_active())
    }

    pub fn any_running(&self) -> bool {
        self.run.as_ref().is_some_and(RunState::is_active)
    }

    pub fn run_for(&self, session: usize) -> Option<&RunState> {
        self.run.as_ref().filter(|r| r.session == session)
    }

    fn clear_finished_run(&mut self, session: usize) {
        if self
            .run
            .as_ref()
            .is_some_and(|r| r.session == session && !r.is_active())
        {
            self.run = None;
        }
    }

    pub fn start_run(&mut self, session: usize, cx: &mut Context<Self>) {
        if self.any_running() || session >= self.config.sessions.len() {
            return;
        }
        let items = self.config.sessions[session].items.clone();
        if items.is_empty() {
            return;
        }
        let cancel = CancelToken::new();
        let (tx, mut rx) = futures::channel::mpsc::unbounded();
        let launcher = SystemLauncher {
            terminal: self.config.settings.terminal.clone(),
        };
        let worker_cancel = cancel.clone();
        let count = items.len();
        if let Err(err) = std::thread::Builder::new()
            .name("azura-run".into())
            .spawn(move || {
                engine::run(&items, &launcher, &worker_cancel, |event| {
                    let _ = tx.unbounded_send(event);
                });
            })
        {
            // Out of threads or memory. Say so instead of taking the whole
            // window down with a panic.
            self.notice = Some(i18n::i18n(cx).tf("run.start_failed", &[("why", &err.to_string())]));
            cx.notify();
            return;
        }

        let events = cx.spawn(async move |this, cx| {
            while let Some(event) = rx.next().await {
                if this
                    .update(cx, |this, cx| this.on_run_event(event, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        // Repaint the countdown while waiting between items.
        let ticker = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let keep_going = this
                    .update(cx, |this, cx| {
                        let active = this.any_running();
                        if this.run.as_ref().is_some_and(|r| r.waiting.is_some()) {
                            cx.notify();
                        }
                        active
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
        self.run = Some(RunState {
            session,
            statuses: vec![ItemStatus::Pending; count],
            waiting: None,
            summary: None,
            cancel,
            _tasks: vec![events, ticker],
        });
        cx.notify();
    }

    fn on_run_event(&mut self, event: RunEvent, cx: &mut Context<Self>) {
        let Some(run) = &mut self.run else {
            return;
        };
        match event {
            RunEvent::Item { index, status } => {
                if let Some(slot) = run.statuses.get_mut(index) {
                    *slot = status;
                }
            }
            RunEvent::Waiting { index, duration } => {
                run.waiting = Some(Waiting {
                    index,
                    until: Instant::now() + duration,
                    total: duration,
                });
            }
            RunEvent::WaitEnded { .. } => run.waiting = None,
            RunEvent::Finished(summary) => {
                run.waiting = None;
                run.summary = Some(summary);
                if summary.all_ok() && self.config.settings.close_after_run {
                    cx.spawn(async move |_, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(1200))
                            .await;
                        cx.update(|cx| cx.quit());
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    /// `--run <name>`: opens the named session and starts it. A name that
    /// matches nothing is reported rather than leaving the user staring at a
    /// window that did nothing.
    pub fn run_by_name(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = name.trim().to_lowercase();
        let found = self
            .config
            .sessions
            .iter()
            .position(|s| s.name.trim().to_lowercase() == wanted);
        let Some(ix) = found else {
            self.notice = Some(i18n::i18n(cx).tf("run.no_such_session", &[("name", name)]));
            cx.notify();
            return;
        };
        // The welcome screen asks for a language first; a run cannot start
        // until that is behind us.
        if self.screen == Screen::Welcome {
            self.notice = Some(i18n::t(cx, "run.finish_welcome_first"));
            self.go(Screen::Sessions, window, cx);
            cx.notify();
            return;
        }
        self.go(Screen::Session(ix), window, cx);
        self.start_run(ix, cx);
    }

    pub fn stop_run(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &self.run {
            run.cancel.cancel();
        }
        cx.notify();
    }

    pub fn skip_wait(&mut self, cx: &mut Context<Self>) {
        if let Some(run) = &self.run {
            run.cancel.skip_wait();
        }
        cx.notify();
    }

    pub fn dismiss_run(&mut self, cx: &mut Context<Self>) {
        if !self.any_running() {
            self.run = None;
        }
        cx.notify();
    }
}

/// Lower is better; `None` means no match.
fn match_score(name: &str, query: &str) -> Option<u8> {
    if name.starts_with(query) {
        Some(0)
    } else if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| word.starts_with(query))
    {
        Some(1)
    } else if name.contains(query) {
        Some(2)
    } else {
        // Initials: "vsc" matches "Visual Studio Code".
        let initials: String = name
            .split(|c: char| !c.is_alphanumeric())
            .filter_map(|word| word.chars().next())
            .collect();
        initials.starts_with(query).then_some(3)
    }
}

/// A display name for a command line: the line itself when it is short,
/// otherwise its program name.
fn command_title(line: &str) -> String {
    if line.chars().count() <= 24 {
        return line.to_owned();
    }
    let line = line.trim();
    // The program is the first word, or the whole quoted part if quoted.
    let program = match line.chars().next() {
        Some(q @ ('"' | '\'')) => line[1..].split(q).next().unwrap_or_default(),
        _ => line.split_whitespace().next().unwrap_or(line),
    };
    // Split on both separators: a Windows path is not a path on Linux.
    let file = program.rsplit(['/', '\\']).next().unwrap_or(program);
    let stem = match file.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty()
                && ["exe", "cmd", "bat", "ps1", "sh", "py"]
                    .contains(&ext.to_ascii_lowercase().as_str()) =>
        {
            stem
        }
        _ => file,
    };
    if stem.is_empty() {
        line.to_owned()
    } else {
        stem.to_owned()
    }
}

fn open_title(target: &azura_platform::OpenTarget) -> String {
    match target {
        azura_platform::OpenTarget::Url(url) => url
            .split("://")
            .nth(1)
            .unwrap_or(url)
            .trim_start_matches("www.")
            .split(['/', '?', '#'])
            .next()
            .unwrap_or(url)
            .to_owned(),
        azura_platform::OpenTarget::Path(path) => azura_platform::name_for_path(path),
    }
}

/// Persian uses the bundled Vazirmatn; other languages use the system UI font.
///
/// The names are the ones GNOME, KDE and the common distributions actually
/// ship. `.SystemUIFont` asks the platform, which is the right answer when
/// none of them is installed.
fn pick_font(code: &str, cx: &App) -> SharedString {
    if code == "fa" {
        return crate::assets::PERSIAN_FONT.into();
    }
    let installed = cx.text_system().all_font_names();
    let has = |name: &str| installed.iter().any(|f| f == name);
    const CANDIDATES: &[&str] = &[
        "Inter",
        "Cantarell",
        "Noto Sans",
        "Ubuntu",
        "DejaVu Sans",
        "Liberation Sans",
    ];
    CANDIDATES
        .iter()
        .find(|name| has(name))
        .map_or(".SystemUIFont".into(), |name| SharedString::from(*name))
}

/// A monospace font for paths and command lines, from the fonts a Linux
/// desktop actually has. Falls back to the UI font rather than guessing a
/// name that is not installed.
pub fn pick_mono_font(cx: &App) -> SharedString {
    let installed = cx.text_system().all_font_names();
    const CANDIDATES: &[&str] = &[
        "JetBrains Mono",
        "Cascadia Mono",
        "Ubuntu Mono",
        "DejaVu Sans Mono",
        "Liberation Mono",
        "Noto Sans Mono",
    ];
    CANDIDATES
        .iter()
        .copied()
        .find(|name| installed.iter().any(|f| f == name))
        .map_or_else(|| pick_font("en", cx), SharedString::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_search_matches() {
        assert_eq!(match_score("telegram desktop", "tele"), Some(0));
        assert_eq!(match_score("visual studio code", "studio"), Some(1));
        assert_eq!(match_score("notepad++", "pad"), Some(2));
        assert_eq!(match_score("visual studio code", "vsc"), Some(3));
        assert_eq!(match_score("telegram", "xyz"), None);
        assert_eq!(
            match_score("cisco anyconnect secure mobility client", "telel"),
            None
        );
    }

    #[test]
    fn derives_titles() {
        assert_eq!(command_title("9router"), "9router");
        assert_eq!(command_title("npm run dev"), "npm run dev");
        assert_eq!(
            command_title("\"C:\\Tools\\srv.exe\" --port 8080 --verbose"),
            "srv"
        );
        assert_eq!(
            command_title("\"C:\\Program Files\\Tool\\run.cmd\" --all --things"),
            "run"
        );
        assert_eq!(
            command_title("/usr/local/bin/server.sh --port 8080 --verbose"),
            "server"
        );
        assert_eq!(
            open_title(&azura_platform::OpenTarget::Url(
                "https://www.github.com/x?y".into()
            )),
            "github.com"
        );
    }
}
