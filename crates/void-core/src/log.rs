/// Timestamped status line macro for human-readable stderr output.
///
/// Produces lines like:
/// ```text
/// 2026-04-28T05:55:30Z [slack:slack] Socket Mode connected
/// ```
///
/// All status output (progress bars, connector events, hooks) should use this
/// instead of raw `eprintln!` so that every line carries a UTC timestamp for
/// easier debugging.
#[macro_export]
macro_rules! status {
    ($($arg:tt)*) => {{
        let __ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
        eprintln!("{__ts}  {}", format_args!($($arg)*));
    }};
}
