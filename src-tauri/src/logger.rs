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

    // PROBLEM 195 — the Sentry bridge WRAPS log4rs rather than replacing it.
    //
    // This used to be one line: `log4rs::init_config(config)`, which builds the
    // logger AND installs it as the global `log::Log`. Only one thing can be
    // the global logger, so bolting Sentry on afterwards is not possible — it
    // has to be `log4rs::Logger::new(config)` (the same logger, not installed)
    // handed to `SentryLogger::with_dest`, which forwards EVERY record to
    // log4rs unchanged and additionally offers it to `telemetry::log_filter`.
    //
    // Consequence worth stating plainly: debug.log is completely unaffected by
    // any of this. Same appender, same pattern, same rotation, same levels.
    // Sentry only ever sees a copy, and only of what `log_filter` lets through
    // (ERROR and above, and only while the user has not switched sending off).
    //
    // `init_config` also set the max level for us; doing this by hand means
    // setting it by hand, or every record below Info is filtered out by `log`
    // itself before either destination sees it.
    let logger = log4rs::Logger::new(config);
    let max_level = logger.max_log_level();
    let bridged = sentry_log::SentryLogger::with_dest(logger)
        .filter(|metadata| crate::telemetry::log_filter(metadata));

    // Ignore error if already initialised (e.g., during tests)
    let _ = log::set_boxed_logger(Box::new(bridged));
    log::set_max_level(max_level);

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
