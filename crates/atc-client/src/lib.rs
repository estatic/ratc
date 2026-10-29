//! Virtual ATC client: X-Plane 11 reader, server connection, push-to-talk, audio and the
//! text mode.

pub mod app;
pub mod audio;
pub mod capture;
pub mod config;
pub mod connection;
pub mod datarefs;
pub mod overlay;
pub mod playback;
pub mod ptt;
pub mod record;
pub mod xplane;

pub use xplane::{
    SimClock, XPlaneState, XpConfig, XpError, XpUpdate, decode_rref, encode_rref_request,
    run_xplane,
};
