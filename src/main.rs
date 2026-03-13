/// PitchBrick entry point.
///
/// Parses CLI arguments, initializes logging, computes default window size
/// from screen metrics, loads configuration, and launches the Iced GUI.
mod app;
mod audio;
mod config;
mod tray;
mod ui;

use clap::Parser;
use iced::window;
use iced::Size;
use std::sync::{Arc, Mutex};

/// Command-line arguments for PitchBrick.
#[derive(Parser)]
#[command(
    name = "pitchbrick",
    about = "Transgender vocal training pitch monitor"
)]
struct Cli {
    /// Enable verbose logging to ~/pitchbrick-verbose.log and show a live log window
    #[arg(short, long)]
    verbose: bool,
}

/// Tracing subscriber layer that forwards formatted log lines to an mpsc channel.
///
/// Only captures events from the `pitchbrick` target (same scope as the file logger).
/// Lines are formatted as `HH:MM:SS.mmm LEVEL message`.
struct LogCaptureLayer {
    tx: std::sync::mpsc::SyncSender<String>,
}

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for LogCaptureLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let meta = event.metadata();
        if !meta.target().starts_with("pitchbrick") {
            return;
        }

        let d = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        let s = d.as_secs();
        let time = format!(
            "{:02}:{:02}:{:02}.{:03}",
            (s / 3600) % 24,
            (s / 60) % 60,
            s % 60,
            d.subsec_millis()
        );

        let level = match *meta.level() {
            tracing::Level::ERROR => "ERROR",
            tracing::Level::WARN => " WARN",
            tracing::Level::INFO => " INFO",
            tracing::Level::DEBUG => "DEBUG",
            tracing::Level::TRACE => "TRACE",
        };

        let mut msg = String::new();
        struct Visitor<'a>(&'a mut String);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_debug(
                &mut self,
                field: &tracing::field::Field,
                value: &dyn std::fmt::Debug,
            ) {
                if field.name() == "message" {
                    use std::fmt::Write;
                    let _ = write!(self.0, "{:?}", value);
                }
            }
        }
        event.record(&mut Visitor(&mut msg));

        let _ = self.tx.send(format!("{} {} {}", time, level, msg));
    }
}

fn main() -> iced::Result {
    let cli = Cli::parse();

    let log_rx = if cli.verbose {
        let (tx, rx) = std::sync::mpsc::sync_channel(4096);
        init_tracing(true, Some(tx));
        Some(rx)
    } else {
        init_tracing(false, None);
        None
    };

    let (screen_w, screen_h) = get_screen_size();
    let side = (0.006 * screen_w as f64 * screen_h as f64).sqrt() as f32;
    tracing::info!(
        "Screen {}x{}, window side: {:.0}px",
        screen_w,
        screen_h,
        side
    );

    let config = config::Config::load(&config::Config::path());
    let main_width = config.window_width.unwrap_or(side);
    let main_height = config.window_height.unwrap_or(side);
    let main_position = match (config.window_x, config.window_y) {
        (Some(x), Some(y)) => window::Position::Specific(iced::Point::new(x as f32, y as f32)),
        _ => window::Position::Default,
    };

    let verbose = cli.verbose;
    let log_rx_arc = Arc::new(Mutex::new(log_rx));
    let log_rx_for_app = Arc::clone(&log_rx_arc);

    // Use daemon() for multi-window support. The main window and optional log
    // window are both opened from PitchBrick::new() via window::open() tasks.
    iced::daemon(
        move || {
            app::PitchBrick::new(
                config.clone(),
                verbose,
                log_rx_for_app.lock().unwrap().take(),
                Size::new(main_width, main_height),
                main_position,
            )
        },
        app::PitchBrick::update,
        app::PitchBrick::view,
    )
    .title(app::PitchBrick::title)
    .subscription(app::PitchBrick::subscription)
    .theme(app::PitchBrick::theme)
    .run()
}

/// Initializes the tracing/logging subsystem.
///
/// In verbose mode, attaches:
/// - A file layer (DEBUG+ → ~/pitchbrick-verbose.log)
/// - A capture layer (DEBUG+ → in-app log window via channel)
/// Always attaches a stderr layer for WARN+.
fn init_tracing(verbose: bool, log_tx: Option<std::sync::mpsc::SyncSender<String>>) {
    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{fmt, EnvFilter};

    let file_layer = if verbose {
        let log_path = dirs::home_dir()
            .expect("Could not determine home directory")
            .join("pitchbrick-verbose.log");
        std::fs::File::create(&log_path).ok().map(|file| {
            fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_filter(EnvFilter::new("pitchbrick=debug"))
        })
    } else {
        None
    };

    let capture_layer = log_tx.map(|tx| LogCaptureLayer { tx });

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(EnvFilter::new("pitchbrick=warn"));

    tracing_subscriber::registry()
        .with(file_layer)
        .with(capture_layer)
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
