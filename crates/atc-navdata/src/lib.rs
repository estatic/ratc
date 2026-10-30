//! X-Plane navdata model and apt.dat parser; `xplane` finds the airport in an X-Plane install.

pub mod apt;
pub mod cifp;
pub mod enroute;
pub mod fixes;
pub mod model;
pub mod procedures;
pub mod station;
pub mod xplane;

pub use apt::{NavdataError, parse_apt_dat};
