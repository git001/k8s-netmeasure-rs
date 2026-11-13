//! Helpers shared by more than one module (design §2, shared-code rule): durations
//! and timestamps, range-checked values, `KEY=VALUE` lists, absolute-or-relative
//! amounts. No I/O, no `unsafe`.
#![forbid(unsafe_code)]
#![forbid(clippy::too_many_arguments)]
