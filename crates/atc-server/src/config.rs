//! `config/server.toml`: listen address, airport data, models. Pure parsing and checks, no
//! speech engines (those are built in `main`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use atc_core::engine::LanguageMode;
use atc_phraseology::Lang;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum StrictnessSetting {
    #[default]
    Standard,
    Training,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WhisperConfig {
    pub model: PathBuf,
    /// `"en"` or `"pl"` fixes the language; `None` = auto-detect (per aircraft, see
    /// [`LanguageMode::Auto`]).
    #[serde(default)]
    pub language: Option<String>,
}

impl WhisperConfig {
    /// The engine's language mode: `Fixed` for `"en"` / `"pl"`, `Auto` when omitted.
    pub fn language_mode(&self) -> Result<LanguageMode> {
        Ok(match self.language.as_deref() {
            None => LanguageMode::Auto,
            Some("en") => LanguageMode::Fixed(Lang::En),
            Some("pl") => LanguageMode::Fixed(Lang::Pl),
            Some(other) => {
                bail!("[whisper] language {other:?}: expected \"en\" or \"pl\", or omit it")
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiperConfig {
    pub bin: PathBuf,
    pub voices: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerConfig {
    pub listen: String,
    /// The one station to run; omitted = every station directory of `data_dir`.
    #[serde(default)]
    pub airport: Option<String>,
    pub data_dir: PathBuf,
    /// X-Plane install whose scenery supplies the airport layout (apt.dat); `None` = the repo
    /// copy in `data_dir`.
    #[serde(default)]
    pub xplane_root: Option<PathBuf>,
    #[serde(default)]
    pub strictness: StrictnessSetting,
    pub whisper: WhisperConfig,
    pub piper: PiperConfig,
}

pub fn load_config(text: &str) -> Result<ServerConfig> {
    let cfg: ServerConfig = toml::from_str(text).context("invalid server config")?;
    cfg.whisper.language_mode()?;
    Ok(cfg)
}

/// `Err` names every configured model file that does not exist, with the script that fetches
/// it.
pub fn check_models(cfg: &ServerConfig) -> Result<(), String> {
    let wanted: [(&Path, &str, &str); 3] = [
        (
            &cfg.whisper.model,
            "whisper model",
            "tools/fetch-whisper.sh",
        ),
        (&cfg.piper.bin, "piper binary", "tools/fetch-piper.sh"),
        (
            &cfg.piper.voices,
            "piper voices config",
            "tools/fetch-piper.sh",
        ),
    ];
    let missing: Vec<String> = wanted
        .iter()
        .filter(|(path, _, _)| !path.is_file())
        .map(|(path, what, script)| format!("{what} not found: {} (run {script})", path.display()))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(missing.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../../config/server.example.toml");

    #[test]
    fn config_parses_example() {
        let c = load_config(EXAMPLE).unwrap();
        assert_eq!(c.listen, "127.0.0.1:8740");
        assert_eq!(c.airport.as_deref(), Some("EPKK"));
        assert_eq!(c.data_dir, PathBuf::from("data/airports"));
        assert_eq!(c.xplane_root, None);
        assert_eq!(c.strictness, StrictnessSetting::Standard);
        assert_eq!(
            c.whisper.model,
            PathBuf::from("models/whisper/ggml-large-v3-turbo-q5_0.bin")
        );
        assert_eq!(c.whisper.language.as_deref(), Some("en"));
        assert_eq!(c.piper.bin, PathBuf::from("models/piper/piper/piper"));
        assert_eq!(c.piper.voices, PathBuf::from("config/voices.toml"));
    }

    #[test]
    fn config_language_and_training() {
        let text = EXAMPLE
            .replace("standard\"", "training\"")
            .replace("language = \"en\"", "language = \"pl\"");
        let c = load_config(&text).unwrap();
        assert_eq!(c.strictness, StrictnessSetting::Training);
        assert_eq!(c.whisper.language.as_deref(), Some("pl"));
        assert_eq!(
            c.whisper.language_mode().unwrap(),
            LanguageMode::Fixed(Lang::Pl)
        );
        assert!(load_config("listen = 1").is_err());
        let without_xp: String = EXAMPLE
            .lines()
            .filter(|l| !l.starts_with("xplane_root"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(load_config(&without_xp).unwrap().xplane_root, None);
    }

    #[test]
    fn language_setting_maps_to_language_mode() {
        let c = load_config(EXAMPLE).unwrap();
        assert_eq!(
            c.whisper.language_mode().unwrap(),
            LanguageMode::Fixed(Lang::En)
        );
        let auto: String = EXAMPLE
            .lines()
            .filter(|l| !l.starts_with("language"))
            .map(|l| format!("{l}\n"))
            .collect();
        let c = load_config(&auto).unwrap();
        assert_eq!(c.whisper.language, None);
        assert_eq!(c.whisper.language_mode().unwrap(), LanguageMode::Auto);
        let e =
            load_config(&EXAMPLE.replace("language = \"en\"", "language = \"de\"")).unwrap_err();
        assert!(format!("{e:#}").contains("\"de\""), "{e:#}");
    }

    #[test]
    fn missing_model_error_mentions_fetch_script() {
        let mut c = load_config(EXAMPLE).unwrap();
        c.whisper.model = "/nonexistent/whisper.bin".into();
        c.piper.bin = "/nonexistent/piper".into();
        c.piper.voices = "/nonexistent/voices.toml".into();
        let e = check_models(&c).unwrap_err();
        assert!(e.contains("tools/fetch-whisper.sh"), "{e}");
        assert!(e.contains("tools/fetch-piper.sh"), "{e}");
        assert!(e.contains("/nonexistent/whisper.bin"), "{e}");
    }
}
