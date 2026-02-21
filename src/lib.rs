//! rt-app-rs library crate.
//!
//! Exposes the core modules for use by integration tests and the binary.

// Suppress dead-code warnings for modules being incrementally ported.
#[allow(dead_code)]
pub mod affinity;
pub mod args;
#[allow(dead_code)]
pub mod config;
pub mod conversions;
#[allow(dead_code)]
pub mod engine;
#[allow(dead_code)]
pub mod gnuplot;
#[allow(dead_code)]
pub mod syscalls;
#[allow(dead_code)]
pub mod taskgroups;
pub mod types;
pub mod utils;
