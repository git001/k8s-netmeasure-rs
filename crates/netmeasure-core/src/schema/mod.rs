//! Serialised types shared by writer and reader (design §5, §7). Readers use
//! `deny_unknown_fields`: an unknown field is a hard failure (§9.1).

pub mod control;
pub mod nic;
pub mod sourced;
