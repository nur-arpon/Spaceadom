/// logger.rs — Rolling file logger for SpaceToggle OS
/// Writes to %APPDATA%\SpaceToggleOS\debug.log, rotating at 5 MB.

use std::path::PathBuf;

/// Initialise the global logger. Call once at the very start of `run()`.
/// Silently succeeds if the logger has already been initialised (e.g. in tests).
pub fn init(log_dir: &PathBuf) {
    use log::LevelFilter;
    use log4rs::{
        append::rolling_file::{
            policy::compound::{
                roll::fixed_window::FixedWindowRoller,
                trigger::size::SizeTrigger,
                CompoundPolicy,
            },
            RollingFileAppender,
        },
        config::{Appender, Config, Root},
        encode::pattern::PatternEncoder,
    };

    let log_path = log_dir.join("debug.log");
    let roller_path = log_dir
        .join("debug.log.{}")
        .to_string_lossy()
        .into_owned();

    // PROBLEM 87 — these used to be .expect()s, which run BEFORE the panic
    // hook is installed: on a machine where %APPDATA% is unwritable (broken
    // roaming profile, over-zealous AV, disk full) the app died instantly
    // with NO window, NO tray and NO log — indistinguishable from "it never
    // started". A keyboard utility must run without its log rather than not
    // run at all.
    let Ok(roller) = FixedWindowRoller::builder().build(&roller_path, 2) else {
        eprintln!("logger: roller build failed — running WITHOUT file logging");
        return;
    };

    let trigger = SizeTrigger::new(5 * 1024 * 1024); // 5 MB
    let policy = CompoundPolicy::new(Box::new(trigger), Box::new(roller));

    let encoder = PatternEncoder::new("{d(%Y-%m-%d %H:%M:%S%.3f)} [{l}] {t} — {m}{n}");

    let Ok(file_appender) = RollingFileAppender::builder()
        .encoder(Box::new(encoder))
        .build(log_path, Box::new(policy))
    else {
        eprintln!("logger: appender build failed (log dir unwritable?) — running WITHOUT file logging");
        return;
    };

    // Release stays at Info: the hook callback contains log::debug! calls, and
    // enabling them means file I/O inside the hook path (hook-eviction risk).
    let level = if cfg!(debug_assertions) { LevelFilter::Debug } else { LevelFilter::Info };
    let Ok(config) = Config::builder()
        .appender(Appender::builder().build("rolling", Box::new(file_appender)))
        .build(
            Root::builder()
                .appender("rolling")
                .build(level),
        )
    else {
        eprintln!("logger: config build failed — running WITHOUT file logging");
        return;
    };

    // Ignore error if already initialised (e.g., during tests)
    let _ = log4rs::init_config(config);

    log::info!("SpaceToggle OS logger initialised");
}

/// Returns the canonical log directory, creating it if necessary.
pub fn log_dir() -> PathBuf {
    let base = dirs_or_appdata();
    std::fs::create_dir_all(&base).ok();
    base
}

fn dirs_or_appdata() -> PathBuf {
    // %APPDATA%\Spaceadom
    std::env::var("APPDATA")
        .map(|p| PathBuf::from(p).join("Spaceadom"))
        .unwrap_or_else(|_| {
            std::env::current_exe()
                .unwrap_or_default()
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .to_path_buf()
        })
}
