//! Reusable building blocks for Delphin-compatible terminal supervisors.
//!
//! The binary is assembled from these modules, and external crates can use the
//! same decision types and [`arbiter::Arbiter`] trait to implement policies
//! without forking the command-line application.

pub mod arbiter;
pub mod config;
pub mod memory;
pub mod queue;
pub mod replay;
pub mod supervisor;
