#[cfg(any(windows, target_os = "macos", target_os = "linux"))]
pub mod app;
pub mod core;
pub mod platform;

#[cfg(test)]
extern crate self as rustboard;
#[cfg(test)]
#[path = "../tests/support/pattern.rs"]
mod test_pattern;
