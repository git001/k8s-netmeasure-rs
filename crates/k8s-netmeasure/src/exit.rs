//! Exit codes (design §8).

use std::process::ExitCode;

pub const OK: u8 = 0;
pub const INTERNAL: u8 = 1;
pub const USAGE: u8 = 2;
/// Refused to start: a self-check or a bind failed.
pub const REFUSED: u8 = 3;
/// `run` completed, at least one cycle invalid.
pub const INVALID_CYCLES: u8 = 4;
/// `report`: the comparison was refused (context mismatch, design contradicted).
pub const COMPARISON_REFUSED: u8 = 5;
pub const SELFTEST_INCOMPLETE: u8 = 6;
pub const PROBE_NOT_OK: u8 = 7;

pub fn code(c: u8) -> ExitCode {
    ExitCode::from(c)
}
