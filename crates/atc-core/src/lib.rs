//! Deterministic ATC core. No I/O, no global state.

pub mod atis;
pub mod compliance;
pub mod conflict;
pub mod engine;
pub mod handover;
pub mod ifr;
pub mod phase;
pub mod readback;
pub mod runway;
pub mod squawk;
pub mod state;
pub mod strip;
pub mod taxi;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
