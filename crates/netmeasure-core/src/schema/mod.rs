//! Serialised types shared by writer and reader (design §5, §7). Readers use
//! `deny_unknown_fields`: an unknown field is a hard failure (§9.1).

pub mod control;
pub mod cycle;
pub mod diagnostics;
pub mod json;
pub mod nic;
pub mod result;
pub mod sourced;
