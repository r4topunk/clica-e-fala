use std::sync::OnceLock;

pub fn ts() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

pub fn verbose() -> bool {
    static VERBOSE: OnceLock<bool> = OnceLock::new();
    *VERBOSE.get_or_init(|| {
        std::env::var("CEF_LOG")
            .map(|v| !v.is_empty() && v != "0" && v.to_lowercase() != "false")
            .unwrap_or(false)
    })
}

/// Verbose log. Suppressed unless `CEF_LOG=1` (or any truthy value).
/// Use for routine state transitions, debug info, traces.
#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => {
        if $crate::logging::verbose() {
            eprintln!("[{}] {}", $crate::logging::ts(), format!($($arg)*))
        }
    };
}

/// Always-on log. Use for errors and unrecoverable conditions.
#[macro_export]
macro_rules! logerr {
    ($($arg:tt)*) => {
        eprintln!("[{}] {}", $crate::logging::ts(), format!($($arg)*))
    };
}
