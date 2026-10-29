//! `voices.toml` parsing: (`VoiceId`, language) -> Piper voice. Pure, no engine needed.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::{VoiceError, VoiceId};

#[derive(Debug, Clone, PartialEq)]
pub struct PiperVoice {
    pub model: PathBuf,
    pub length_scale: f32,
}

#[derive(Deserialize)]
struct RawVoice {
    model: PathBuf,
    #[serde(default = "default_length_scale")]
    length_scale: f32,
}

fn default_length_scale() -> f32 {
    1.0
}

/// A voice and the language it speaks (ISO code, e.g. `"en"`).
pub type VoiceKey = (VoiceId, String);

fn voice_id(name: &str) -> Result<VoiceId, VoiceError> {
    match name {
        "delivery" => Ok(VoiceId::Delivery),
        "ground" => Ok(VoiceId::Ground),
        "tower" => Ok(VoiceId::Tower),
        "approach" => Ok(VoiceId::Approach),
        "radar" => Ok(VoiceId::Radar),
        "atis" => Ok(VoiceId::Atis),
        other => Err(VoiceError::Engine(format!("unknown voice id: {other}"))),
    }
}

fn voice(v: toml::Value, base: &Path) -> Result<PiperVoice, VoiceError> {
    let v: RawVoice = v
        .try_into()
        .map_err(|e| VoiceError::Engine(format!("voices config: {e}")))?;
    Ok(PiperVoice {
        model: base.join(v.model),
        length_scale: v.length_scale,
    })
}

/// Parse `voices.toml`; model paths are resolved against `base` (the repo root). Top-level
/// `[delivery]`, `[ground]`, `[tower]`, `[approach]`, `[radar]` and `[atis]` are the English
/// voices; `[<lang>.<voice>]` (a two-letter language code, e.g. `[pl.tower]`) the voices of
/// another language.
pub fn load_voices_config(
    text: &str,
    base: &Path,
) -> Result<HashMap<VoiceKey, PiperVoice>, VoiceError> {
    let raw = toml::from_str::<toml::Table>(text)
        .map_err(|e| VoiceError::Engine(format!("voices config: {e}")))?;
    let mut out = HashMap::new();
    for (k, v) in raw {
        let is_lang = k.len() == 2 && k.bytes().all(|b| b.is_ascii_lowercase());
        if !is_lang {
            out.insert((voice_id(&k)?, "en".to_string()), voice(v, base)?);
            continue;
        }
        let toml::Value::Table(voices) = v else {
            return Err(VoiceError::Engine(format!(
                "voices config: [{k}] must hold voice tables"
            )));
        };
        for (name, v) in voices {
            out.insert((voice_id(&name)?, k.clone()), voice(v, base)?);
        }
    }
    Ok(out)
}

/// The voice `id` speaking `lang`, or else its English voice; the flag is `true` for that
/// fallback. `None` when neither is configured.
pub fn pick_voice<'a, V>(
    voices: &'a HashMap<VoiceKey, V>,
    id: VoiceId,
    lang: &str,
) -> Option<(&'a V, bool)> {
    if let Some(v) = voices.get(&(id, lang.to_string())) {
        return Some((v, false));
    }
    voices.get(&(id, "en".to_string())).map(|v| (v, true))
}

/// The voice that speaks for `id`: `id` itself, or the Ground voice for Delivery, Approach
/// and Radar when `voices.toml` has no entry for them in any language.
pub fn ground_if_unconfigured<V>(voices: &HashMap<VoiceKey, V>, id: VoiceId) -> VoiceId {
    if id.falls_back_to_ground() && !voices.keys().any(|(v, _)| *v == id) {
        VoiceId::Ground
    } else {
        id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: VoiceId, lang: &str) -> (VoiceId, String) {
        (id, lang.to_string())
    }

    #[test]
    fn voices_config_parses() {
        let text = include_str!("../../../config/voices.toml");
        let m = load_voices_config(text, Path::new("/repo")).unwrap();
        let g = &m[&key(VoiceId::Ground, "en")];
        assert_eq!(
            g.model,
            Path::new("/repo/models/piper/voices/en_US-ryan-medium.onnx")
        );
        assert!((g.length_scale - 0.95).abs() < 1e-6);
        assert!(m.contains_key(&key(VoiceId::Tower, "en")));
        assert!(m.contains_key(&key(VoiceId::Atis, "en")));
    }

    #[test]
    fn voices_config_reads_polish_tables() {
        let text = include_str!("../../../config/voices.toml");
        let m = load_voices_config(text, Path::new("/repo")).unwrap();
        assert_eq!(m.len(), 10);
        for (id, name) in [
            (VoiceId::Delivery, "pl_PL-darkman-medium"),
            (VoiceId::Ground, "pl_PL-darkman-medium"),
            (VoiceId::Tower, "pl_PL-mc_speech-medium"),
            (VoiceId::Atis, "pl_PL-gosia-medium"),
        ] {
            assert_eq!(
                m[&key(id, "pl")].model,
                Path::new(&format!("/repo/models/piper/voices/{name}.onnx")),
                "{id:?}"
            );
        }
    }

    #[test]
    fn voices_config_reads_the_english_controller_voices() {
        let text = include_str!("../../../config/voices.toml");
        let m = load_voices_config(text, Path::new("/repo")).unwrap();
        for (id, name) in [
            (VoiceId::Delivery, "en_US-lessac-medium"),
            (VoiceId::Approach, "en_GB-northern_english_male-medium"),
            (VoiceId::Radar, "en_US-joe-medium"),
        ] {
            assert_eq!(
                m[&key(id, "en")].model,
                Path::new(&format!("/repo/models/piper/voices/{name}.onnx")),
                "{id:?}"
            );
        }
        // Approach and the adjacent units speak English only.
        assert!(!m.contains_key(&key(VoiceId::Approach, "pl")));
        assert!(!m.contains_key(&key(VoiceId::Radar, "pl")));
    }

    #[test]
    fn missing_polish_voice_falls_back_to_english() {
        let text = "[tower]\nmodel = \"en.onnx\"\n[pl.ground]\nmodel = \"pl.onnx\"\n";
        let m = load_voices_config(text, Path::new("/r")).unwrap();
        let (v, fallback) = pick_voice(&m, VoiceId::Tower, "pl").unwrap();
        assert_eq!(
            (v.model.as_path(), fallback),
            (Path::new("/r/en.onnx"), true)
        );
        let (v, fallback) = pick_voice(&m, VoiceId::Tower, "en").unwrap();
        assert_eq!(
            (v.model.as_path(), fallback),
            (Path::new("/r/en.onnx"), false)
        );
        // A Polish voice is used as is; no English one to fall back to is fine.
        let (v, fallback) = pick_voice(&m, VoiceId::Ground, "pl").unwrap();
        assert_eq!(
            (v.model.as_path(), fallback),
            (Path::new("/r/pl.onnx"), false)
        );
        assert!(pick_voice(&m, VoiceId::Atis, "pl").is_none());
        assert!(pick_voice(&m, VoiceId::Ground, "en").is_none());
    }

    #[test]
    fn unconfigured_positions_speak_with_the_ground_voice() {
        let text = "[ground]\nmodel = \"g.onnx\"\n[approach]\nmodel = \"a.onnx\"\n";
        let m = load_voices_config(text, Path::new("/r")).unwrap();
        assert_eq!(
            ground_if_unconfigured(&m, VoiceId::Delivery),
            VoiceId::Ground
        );
        assert_eq!(ground_if_unconfigured(&m, VoiceId::Radar), VoiceId::Ground);
        assert_eq!(
            ground_if_unconfigured(&m, VoiceId::Approach),
            VoiceId::Approach
        );
        assert_eq!(ground_if_unconfigured(&m, VoiceId::Tower), VoiceId::Tower);
    }

    #[test]
    fn unknown_voice_is_error() {
        let r = load_voices_config("[bogus]\nmodel = \"x\"\n", Path::new("."));
        assert!(matches!(r, Err(VoiceError::Engine(_))));
        let r = load_voices_config("[pl.bogus]\nmodel = \"x\"\n", Path::new("."));
        assert!(matches!(r, Err(VoiceError::Engine(_))));
    }
}
