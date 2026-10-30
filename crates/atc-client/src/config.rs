//! `config/client.toml`.

use std::path::Path;

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    /// WebSocket URL of the server.
    pub server: String,
    /// Always fly under this callsign; absent: the X-Plane tail number.
    pub callsign: Option<String>,
    /// X-Plane host:port (RREF, normally port 49000).
    pub xplane: String,
    /// Ignored (the aircraft type comes from X-Plane); accepted so old files still load.
    aircraft_type: Option<toml::Value>,
    /// Ignored (cloud bases are sent above sea level); accepted so old files still load.
    field_elev_ft: Option<toml::Value>,
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
        let src = std::fs::read_to_string(path).with_context(|| missing_hint("reading", path))?;
        let cfg: Self =
            toml::from_str(&src).with_context(|| format!("parsing {}", path.display()))?;
        for w in cfg.warnings() {
            tracing::warn!("{w}");
        }
        Ok(cfg)
    }

    /// One warning per key that is still in the file but no longer used.
    pub fn warnings(&self) -> Vec<String> {
        [
            ("aircraft_type", self.aircraft_type.is_some()),
            ("field_elev_ft", self.field_elev_ft.is_some()),
        ]
        .into_iter()
        .filter(|(_, present)| *present)
        .map(|(key, _)| {
            format!(
                "config/client.toml: '{key}' is no longer used (read from X-Plane); remove the line"
            )
        })
        .collect()
    }
}

/// `"<verb> <path>"`, plus how to create the file from its `.example.toml` when it is missing.
fn missing_hint(verb: &str, path: &Path) -> String {
    let example = path.with_extension("example.toml");
    if !path.exists() && example.exists() {
        format!(
            "{verb} {}: not found; copy {} to {} and edit it",
            path.display(),
            example.display(),
            path.display()
        )
    } else {
        format!("{verb} {}", path.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The repo's config parses; its values are the user's to edit, so none are pinned.
    #[test]
    fn repo_client_toml_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/client.example.toml");
        let c = ClientConfig::load(&path).unwrap();
        assert!(c.warnings().is_empty());
    }

    #[test]
    fn example_config_parses() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/client.example.toml");
        let c = ClientConfig::load(&path).unwrap();
        assert_eq!(c.callsign, None);
    }

    #[test]
    fn old_keys_load_with_warning() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/client.example.toml");
        let src = std::fs::read_to_string(path).unwrap();
        let old = format!("aircraft_type = \"C172\"\nfield_elev_ft = 791\n{src}");
        let c: ClientConfig = toml::from_str(&old).unwrap();
        assert_eq!(
            c.warnings(),
            [
                "config/client.toml: 'aircraft_type' is no longer used (read from X-Plane); remove the line",
                "config/client.toml: 'field_elev_ft' is no longer used (read from X-Plane); remove the line",
            ]
        );
    }

    #[test]
    fn unknown_keys_still_fail() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/client.example.toml");
        let src = std::fs::read_to_string(path).unwrap();
        assert!(toml::from_str::<ClientConfig>(&format!("bogus = 1\n{src}")).is_err());
    }

    #[test]
    fn missing_config_names_its_example() {
        let dir = std::env::temp_dir().join(format!("atc-client-hint-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("client.example.toml"), "").unwrap();
        let err = ClientConfig::load(&dir.join("client.toml")).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("client.example.toml"), "{msg}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
