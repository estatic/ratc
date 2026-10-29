//! `config/client.toml`.

use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    /// WebSocket URL of the server.
    pub server: String,
    pub callsign: String,
    pub aircraft_type: String,
    /// X-Plane host:port (RREF, normally port 49000).
    pub xplane: String,
    /// Field elevation for cloud bases above ground in the weather report.
    #[serde(default)]
    pub field_elev_ft: f64,
    /// host:port of the X-Plane overlay plugin; [`crate::overlay::DEFAULT_ADDR`] when absent.
    pub overlay: Option<String>,
    pub ptt: PttConfig,
    /// Substring of the capture device name; the default device when absent.
    pub input_device: Option<String>,
    /// Substring of the playback device name; the default device when absent.
    pub output_device: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PttConfig {
    /// Joystick button number (0 = trigger); omit to use only the key.
    pub joystick_button: Option<u32>,
    /// `"space"`, `"enter"`, `"tab"` or one character.
    pub key: String,
}

impl ClientConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let src =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&src).with_context(|| format!("parsing {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo's config parses; its values are the user's to edit, so none are pinned.
    #[test]
    fn repo_client_toml_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/client.toml");
        let c = ClientConfig::load(&path).unwrap();
        assert!(!c.callsign.is_empty());
    }
}
