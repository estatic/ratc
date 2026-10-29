//! Mono 16-bit WAV I/O.

use crate::{Pcm, VoiceError};
use std::path::Path;

fn io<E: std::fmt::Display>(e: E) -> VoiceError {
    VoiceError::Io(e.to_string())
}

pub fn read_wav(path: impl AsRef<Path>) -> Result<Pcm, VoiceError> {
    let mut r = hound::WavReader::open(path).map_err(io)?;
    let spec = r.spec();
    if spec.channels != 1
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(VoiceError::Io(format!(
            "expected mono 16-bit PCM wav, got {} ch / {} bit",
            spec.channels, spec.bits_per_sample
        )));
    }
    let samples = r
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(io)?;
    Ok(Pcm {
        sample_rate: spec.sample_rate,
        samples,
    })
}

pub fn write_wav(path: impl AsRef<Path>, pcm: &Pcm) -> Result<(), VoiceError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: pcm.sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).map_err(io)?;
    for &s in &pcm.samples {
        w.write_sample(s).map_err(io)?;
    }
    w.finalize().map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_roundtrip() {
        let pcm = Pcm {
            sample_rate: 22050,
            samples: vec![0, 1, -1, 32767, -32768, 1234],
        };
        let path = std::env::temp_dir().join(format!("atc-voice-rt-{}.wav", std::process::id()));
        write_wav(&path, &pcm).unwrap();
        let back = read_wav(&path).unwrap();
        std::fs::remove_file(&path).ok();
        assert_eq!(back, pcm);
    }

    #[test]
    fn read_missing_file_is_io_error() {
        assert!(matches!(
            read_wav("/nonexistent/x.wav"),
            Err(VoiceError::Io(_))
        ));
    }
}
