//! Measurement, validity, schema, statistics and report model (design §2).
//!
//! This crate does no I/O at all: no sockets, no files, no clock reads, no logging.
//! Everything in it is therefore unit-testable.
#![forbid(unsafe_code)]
#![forbid(clippy::too_many_arguments)]

pub mod diag;
pub mod measure;
pub mod schema;
pub mod stats;
