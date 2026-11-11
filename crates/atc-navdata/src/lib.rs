//! Simulator-neutral navdata: the airport/procedure model, the apt.dat and CIFP parsers, and
//! `source`, the trait through which the server reads a simulator's data. `xplane` is the
//! X-Plane source (finds the airport in an X-Plane install).

pub mod apt;
pub mod cifp;
pub mod enroute;
pub mod fixes;
pub mod model;
pub mod procedures;
pub mod source;
pub mod station;
pub mod xplane;

pub use apt::{NavdataError, parse_apt_dat};
