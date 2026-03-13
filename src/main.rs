/// PitchBrick entry point.
///
/// Parses CLI arguments, initializes logging, computes default window size
/// from screen metrics, loads configuration, and launches the Iced GUI.
mod app;
mod audio;
mod config;
mod ui;

use clap::Parser;
use iced::window;
use iced::Size;

/// Command-line arguments for PitchBrick.
#[derive(Parser)]
#[command(
    name = "pitchbrick",
    about = "Transgender vocal training pitch monitor"
)]
struct Cli {
    /// Enable verbose logging to ~/pitchbrick-verbose.log
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> iced::Result {
    let cli = Cli::parse();
    init_tracing(cli.verbose);

    let (screen_w, screen_h) = get_screen_size();
    let side = (0.006 * screen_w as f64 * screen_h as f64).sqrt() as f32;
    tracing::info!(
        "Screen {}x{}, window side: {:.0}px",
        screen_w,
        screen_h,
        side
    );

    let config = config::Config::load(&config::Config::path());
    let width = config.window_width.unwrap_or(side);
    let height = config.window_height.unwrap_or(side);
    let position = match (config.window_x, config.window_y) {
        (Some(x), Some(y)) => window::Position::Specific(iced::Point::new(x as f32, y as f32)),
        _ => window::Position::Default,
    };

    iced::application(
        move || app::PitchBrick::new(config.clone()),
        app::PitchBrick::update,
        app::PitchBrick::view,
    )
    .title("PitchBrick")
    .subscription(app::PitchBrick::subscription)
    .theme(app::PitchBrick::theme)
    .window_size(Size::new(width, height))
    .position(position)
    .decorations(false)
    .level(window::Level::AlwaysOnTop)
    .resizable(true)
    .run()
}

/// Initializes the tracing/logging subsystem.
///
/// When verbose is true, logs DEBUG+ to ~/pitchbrick-verbose.log (truncated
/// each run) and WARN+ to stderr. When verbose is false, only WARN+ to stderr.
fn init_tracing(verbose: bool) {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    if verbose {
        let log_path = dirs::home_dir()
            .expect("Could not determine home directory")
            .join("pitchbrick-verbose.log");

        if let Ok(file) = std::fs::File::create(&log_path) {
            let file_layer = fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_filter(EnvFilter::new("pitchbrick=debug"));

            let stderr_layer = fmt::layer()
                .with_writer(std::io::stderr)
                .with_filter(EnvFilter::new("pitchbrick=warn"));

            tracing_subscriber::registry()
                .with(file_layer)
                .with(stderr_layer)
                .init();

            tracing::info!("Verbose logging to {:?}", log_path);
            return;
        }
        eprintln!(
            "Warning: could not create verbose log file at {:?}",
            log_path
        );
    }

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(EnvFilter::new("pitchbrick=warn"));

    tracing_subscriber::registry()
        .with(stderr_layer)
        .init();
}

/// Returns the primary screen dimensions in pixels.
#[cfg(windows)]
fn get_screen_size() -> (i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
    unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) }
}

/// Fallback screen size for non-Windows platforms (development only).
#[cfg(not(windows))]
fn get_screen_size() -> (i32, i32) {
    (1920, 1080)
}
