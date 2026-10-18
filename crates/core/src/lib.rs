//! Portable protocol and bounded content sources. No platform API types belong here.
pub mod engine;
pub mod protocol;
pub mod source;
pub mod transport;
pub use protocol::*;
pub use source::*;

#[cfg(test)]
extern crate self as rustboard_core;

#[cfg(test)]
#[path = "../../../tests/support/pattern.rs"]
mod test_pattern;
