//! Piper TTS via subprocess: text on stdin, raw s16le on stdout.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

use crate::{
    Pcm, PiperVoice, TextToSpeech, VoiceError, VoiceId, VoiceKey, ground_if_unconfigured,
    pick_voice,
};

pub struct PiperTts {
    bin: PathBuf,
    voices: HashMap<VoiceKey, (PiperVoice, u32)>,
    /// Voices already reported as falling back to English (one warning each).
    warned: Mutex<HashSet<VoiceKey>>,
}

fn sample_rate_of(model: &Path) -> Result<u32, VoiceError> {
    let mut json = model.as_os_str().to_owned();
    json.push(".json");
    let text = std::fs::read_to_string(&json)
        .map_err(|e| VoiceError::ModelMissing(format!("{}: {e}", Path::new(&json).display())))?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| VoiceError::Engine(format!("model json: {e}")))?;
    v["audio"]["sample_rate"]
        .as_u64()
        .map(|r| r as u32)
        .ok_or_else(|| VoiceError::Engine("model json: no audio.sample_rate".into()))
}

/// Lexically absolute path (no symlink resolution, works for missing files).
fn absolutize(p: &Path) -> Result<PathBuf, VoiceError> {
    std::path::absolute(p).map_err(|e| VoiceError::Io(format!("{}: {e}", p.display())))
}

impl PiperTts {
    /// Checks that the binary, every English model and its `.onnx.json` exist. A voice of
    /// another language whose files are missing is left out (it falls back to English).
    pub fn new(
        piper_bin: PathBuf,
        voices: HashMap<VoiceKey, PiperVoice>,
    ) -> Result<Self, VoiceError> {
        // The subprocess runs with current_dir = the binary's dir, so every path
        // handed to it must be absolute.
        let piper_bin = absolutize(&piper_bin)?;
        if !piper_bin.is_file() {
            return Err(VoiceError::ModelMissing(piper_bin.display().to_string()));
        }
        let mut out = HashMap::new();
        let mut warned = HashSet::new();
        for (key, mut v) in voices {
            v.model = absolutize(&v.model)?;
            let loaded = if v.model.is_file() {
                sample_rate_of(&v.model)
            } else {
                Err(VoiceError::ModelMissing(v.model.display().to_string()))
            };
            match loaded {
                Ok(rate) => {
                    out.insert(key, (v, rate));
                }
                Err(e @ VoiceError::ModelMissing(_)) if key.1 != "en" => {
                    tracing::warn!(
                        %e, voice = ?key.0, lang = %key.1,
                        "voice missing (run tools/fetch-piper.sh, or .ps1 on Windows); using the English one"
                    );
                    warned.insert(key);
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Self {
            bin: piper_bin,
            voices: out,
            warned: Mutex::new(warned),
        })
    }
}

impl TextToSpeech for PiperTts {
    fn synthesize(&self, voice: VoiceId, lang: &str, text: &str) -> Result<Pcm, VoiceError> {
        let spoken_by = ground_if_unconfigured(&self.voices, voice);
        if spoken_by != voice && self.warned.lock().unwrap().insert((voice, String::new())) {
            tracing::warn!(
                ?voice,
                "no voice configured for this position; using the Ground voice"
            );
        }
        let ((v, rate), fallback) = pick_voice(&self.voices, spoken_by, lang).ok_or_else(|| {
            VoiceError::ModelMissing(format!("no voice configured for {voice:?}"))
        })?;
        if fallback
            && self
                .warned
                .lock()
                .unwrap()
                .insert((voice, lang.to_string()))
        {
            tracing::warn!(
                ?voice,
                lang,
                "no voice for this language; using the English one"
            );
        }
        let mut cmd = Command::new(&self.bin);
        // The release bundles espeak-ng data and shared libs next to the binary.
        if let Some(dir) = self.bin.parent() {
            cmd.current_dir(dir);
        }
        let mut child = cmd
            .arg("--model")
            .arg(&v.model)
            .arg("--output_raw")
            .arg("--length_scale")
            .arg(v.length_scale.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| VoiceError::Engine(format!("spawn piper: {e}")))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        // One line per utterance; newlines inside would split it.
        let line = format!("{}\n", text.replace('\n', " "));
        let writer = std::thread::spawn(move || stdin.write_all(line.as_bytes()));
        let out = child
            .wait_with_output()
            .map_err(|e| VoiceError::Engine(format!("wait piper: {e}")))?;
        let _ = writer.join();
        if !out.status.success() {
            return Err(VoiceError::Engine(format!(
                "piper exited with {}: {}",
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let (pairs, _) = out.stdout.as_chunks::<2>();
        let samples = pairs.iter().map(|b| i16::from_le_bytes(*b)).collect();
        Ok(Pcm {
            sample_rate: *rate,
            samples,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_model_is_model_missing() {
        let mut voices = HashMap::new();
        voices.insert(
            (VoiceId::Ground, "en".to_string()),
            PiperVoice {
                model: PathBuf::from("/nonexistent/voice.onnx"),
                length_scale: 1.0,
            },
        );
        let exe = std::env::current_exe().unwrap();
        let r = PiperTts::new(exe, voices);
        assert!(matches!(r, Err(VoiceError::ModelMissing(_))));
        let r = PiperTts::new(PathBuf::from("/nonexistent/piper"), HashMap::new());
        assert!(matches!(r, Err(VoiceError::ModelMissing(_))));
    }

    #[test]
    fn missing_polish_model_is_left_out_and_warned_once() {
        let key = (VoiceId::Tower, "pl".to_string());
        let mut voices = HashMap::new();
        voices.insert(
            key.clone(),
            PiperVoice {
                model: PathBuf::from("/nonexistent/pl.onnx"),
                length_scale: 1.0,
            },
        );
        let tts = PiperTts::new(std::env::current_exe().unwrap(), voices).unwrap();
        assert!(tts.voices.is_empty());
        // Already reported: synthesizing with the fallback does not warn again.
        assert!(tts.warned.lock().unwrap().contains(&key));
    }

    #[test]
    fn relative_paths_are_absolutized() {
        // Everything under the test's cwd (the crate dir), so relative paths exist even when
        // the target dir is on another drive (Windows CI: C:/t vs D:\a\...).
        let name = format!("piper-rel-test-{}", std::process::id());
        let dir = std::env::current_dir().unwrap().join(&name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("piper"), b"").unwrap();
        std::fs::write(dir.join("v.onnx"), b"").unwrap();
        std::fs::write(
            dir.join("v.onnx.json"),
            br#"{"audio":{"sample_rate":22050}}"#,
        )
        .unwrap();
        let mut voices = HashMap::new();
        let key = (VoiceId::Ground, "en".to_string());
        voices.insert(
            key.clone(),
            PiperVoice {
                model: Path::new(&name).join("v.onnx"),
                length_scale: 1.0,
            },
        );
        let tts = PiperTts::new(Path::new(&name).join("piper"), voices);
        std::fs::remove_dir_all(&dir).unwrap();
        let tts = tts.unwrap();
        assert!(tts.bin.is_absolute());
        assert!(tts.voices[&key].0.model.is_absolute());
    }
}
