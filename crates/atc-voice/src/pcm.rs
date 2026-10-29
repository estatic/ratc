//! Mono 16-bit PCM buffer and linear resampling.

#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn duration_s(&self) -> f64 {
        if self.sample_rate == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / self.sample_rate as f64
    }
}

/// Linear-interpolation resampler. Output length is `round(len * to / from)`.
pub fn resample(input: &Pcm, to_rate: u32) -> Pcm {
    if input.sample_rate == to_rate || input.samples.is_empty() {
        return Pcm {
            sample_rate: to_rate,
            samples: input.samples.clone(),
        };
    }
    let n_in = input.samples.len();
    let ratio = input.sample_rate as f64 / to_rate as f64;
    let n_out = (n_in as f64 / ratio).round() as usize;
    let samples = (0..n_out)
        .map(|i| {
            let pos = i as f64 * ratio;
            let idx = pos.floor() as usize;
            let frac = pos - idx as f64;
            let a = input.samples[idx.min(n_in - 1)] as f64;
            let b = input.samples[(idx + 1).min(n_in - 1)] as f64;
            (a + (b - a) * frac).round() as i16
        })
        .collect();
    Pcm {
        sample_rate: to_rate,
        samples,
    }
}

#[cfg(test)]
pub(crate) fn sine(rate: u32, freq: f64, secs: f64, amp: f64) -> Pcm {
    let n = (rate as f64 * secs) as usize;
    let samples = (0..n)
        .map(|i| {
            (amp * 32767.0 * (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin())
                as i16
        })
        .collect();
    Pcm {
        sample_rate: rate,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_48k_to_16k_preserves_duration() {
        let p = sine(48000, 440.0, 1.234, 0.5);
        let r = resample(&p, 16000);
        assert_eq!(r.sample_rate, 16000);
        assert!((r.duration_s() - p.duration_s()).abs() < 0.001);
    }

    #[test]
    fn resample_sine_keeps_frequency() {
        let zc = |p: &Pcm| {
            p.samples
                .windows(2)
                .filter(|w| (w[0] < 0) != (w[1] < 0))
                .count() as f64
                / p.duration_s()
        };
        for rate in [48000, 44100, 22050] {
            let p = sine(rate, 1000.0, 1.0, 0.5);
            let r = resample(&p, 16000);
            let (a, b) = (zc(&p), zc(&r));
            assert!((a - b).abs() / a < 0.02, "rate {rate}: {a} vs {b}");
        }
    }

    #[test]
    fn duration_of_empty_is_zero() {
        assert_eq!(
            Pcm {
                sample_rate: 16000,
                samples: vec![]
            }
            .duration_s(),
            0.0
        );
    }
}
