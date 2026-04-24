pub fn ts() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

#[macro_export]
macro_rules! logln {
    ($($arg:tt)*) => {
        eprintln!("[{}] {}", $crate::logging::ts(), format!($($arg)*))
    };
}
