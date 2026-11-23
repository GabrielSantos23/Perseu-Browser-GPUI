//! GPUI application for perseu: window, sidebar, command bar, settings and theme.

pub mod theme;

mod actions;
mod command_bar;
mod context_menu;
mod default_browser;
mod grain;
mod icons;
mod instance;
mod local_time;
mod loopback;
mod media;
mod motion;
mod native;
mod page;
mod paths;
mod shell;
mod site_menu;
mod theme_picker;
mod title_bar;
mod toast;
mod tooltip;
mod updater;

use std::fs::File;
use std::sync::Mutex;

use gpui::{
    App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
use gpui_kit::component::{Root, theme::ThemeMode};
use tracing_subscriber::EnvFilter;

use shell::Shell;
use theme::Theme;

const DEFAULT_LOG_FILTER: &str = "warn";

/// Starts the GPUI event loop and opens the main browser window. URLs and
/// files given on the command line open as tabs; when another instance is
/// already running they are handed to it and this process exits.
pub fn run() {
    init_logging();
    match webview_host::runtime_version() {
        Some(version) => tracing::info!("WebView2 Runtime {version}"),
        None => {
            native::offer_runtime_install();
            return;
        }
    }
    let targets: Vec<String> = std::env::args()
        .skip(1)
        .filter_map(|argument| instance::accepted_target(&argument))
        .collect();
    // A private window is its own process and never takes over or forwards.
    let private = std::env::args()
        .skip(1)
        .any(|argument| argument == shell::PRIVATE_FLAG);
    if !private && let instance::Launch::Forwarded = instance::claim(&targets) {
        return;
    }
    gpui_platform::application().run(move |cx: &mut App| {
        gpui_kit::init(cx);
        gpui_kit::component::Theme::change(ThemeMode::Dark, None, cx);
        Theme::init(cx);
        theme::apply_to_components(cx);
        command_bar::bind_keys(cx);
        context_menu::bind_keys(cx);
        shell::bind_peek_keys(cx);
        shell::bind_settings_keys(cx);
        shell::bind_history_keys(cx);
        quit_when_last_window_closes(cx);

        if let Err(error) = open_main_window(targets, private, cx) {
            tracing::error!("failed to open main window: {error:#}");
            cx.quit();
            return;
        }
        cx.activate(true);
    });
}

fn quit_when_last_window_closes(cx: &mut App) {
    cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            cx.quit();
        }
    })
    .detach();
}

fn init_logging() {
    let filter =
        || EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    let log_file = (!cfg!(debug_assertions)).then(|| paths::Paths::detect().log_file());
    let file = log_file.and_then(|path| {
        std::fs::create_dir_all(path.parent()?).ok()?;
        File::create(path).ok()
    });
    match file {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter())
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter()).init(),
    }
}

/// The window's place from the last session, while it is still on a
/// connected screen; otherwise centered.
fn saved_window_bounds(cx: &App) -> WindowBounds {
    let saved = browser_core::store::load_json::<browser_core::session::Session>(
        &paths::Paths::detect().session(),
    )
    .ok()
    .flatten()
    .and_then(|session| session.layout.window);
    let centered = WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx));
    let Some(saved) = saved else {
        return centered;
    };
    let bounds = Bounds::new(
        point(px(saved.x), px(saved.y)),
        size(px(saved.width), px(saved.height)),
    );
    if !cx
        .displays()
        .iter()
        .any(|display| display.bounds().intersects(&bounds))
    {
        return centered;
    }
    if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    }
}

fn open_main_window(targets: Vec<String>, private: bool, cx: &mut App) -> anyhow::Result<()> {
    let bounds = if private {
        WindowBounds::Windowed(Bounds::centered(None, size(px(1280.), px(800.)), cx))
    } else {
        saved_window_bounds(cx)
    };
    let options = WindowOptions {
        window_bounds: Some(bounds),
        titlebar: Some(TitlebarOptions {
            title: Some(browser_core::APP_NAME.into()),
            appears_transparent: true,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(640.), px(420.))),
        ..Default::default()
    };
    cx.open_window(options, |window, cx| {
        let shell = cx.new(|cx| Shell::new(targets, private, window, cx));
        cx.new(|cx| Root::new(shell, window, cx))
    })?;
    Ok(())
}
