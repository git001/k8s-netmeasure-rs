//! Exit codes (design §8).

use std::process::ExitCode;

pub const OK: u8 = 0;
pub const INTERNAL: u8 = 1;
pub const USAGE: u8 = 2;
/// Refused to start: a self-check or a bind failed.
pub const REFUSED: u8 = 3;
pub const SELFTEST_INCOMPLETE: u8 = 6;
pub const PROBE_NOT_OK: u8 = 7;

pub fn code(c: u8) -> ExitCode {
    ExitCode::from(c)
}
