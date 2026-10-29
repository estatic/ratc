//! Radio-voice effect: 300-3400 Hz band-pass (HPF + 2x LPF biquads), soft clipping, white noise.

use crate::pcm::Pcm;
use std::f64::consts::PI;

const Q: f64 = 0.707;
const NOISE_DBFS: f64 = -30.0;

#[derive(Clone, Copy)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    z1: f64,
    z2: f64,
}

impl Biquad {
    fn new(high_pass: bool, fc: f64, rate: f64) -> Self {
        let w0 = 2.0 * PI * fc / rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * Q);
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = if high_pass {
            ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0)
        } else {
            ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0)
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// xorshift64* — tiny deterministic PRNG, uniform in [-1, 1).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let v = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (v >> 11) as f64 / (1u64 << 52) as f64 - 1.0
    }
}

/// Applies the radio effect. Deterministic for a given `seed`.
pub fn radio_effect(pcm: &Pcm, seed: u64) -> Pcm {
    let rate = pcm.sample_rate as f64;
    let mut hpf = Biquad::new(true, 300.0, rate);
    // Two cascaded LPF sections: a single 2nd-order section only gives ~10 dB
    // at 6 kHz for 22.05 kHz input (Piper's rate), short of the 12 dB target.
    let mut lpf1 = Biquad::new(false, 3400.0, rate);
    let mut lpf2 = lpf1;
    // Uniform noise in [-a, a] has RMS a/sqrt(3).
    let noise_amp = 10f64.powf(NOISE_DBFS / 20.0) * 3f64.sqrt();
    // Avoid the all-zero xorshift fixed point.
    let mut rng = Rng(seed ^ 0x9E37_79B9_7F4A_7C15 | 1);
    let samples = pcm
        .samples
        .iter()
        .map(|&s| {
            let x = s as f64 / 32768.0;
            let y = hpf.process(x);
            let y = lpf2.process(lpf1.process(y));
            let y = (y * 1.2).tanh() / 1.2_f64.tanh();
            let y = y * 0.9 + rng.next() * noise_amp;
            (y * 32767.0).round().clamp(-32768.0, 32767.0) as i16
        })
        .collect();
    Pcm {
        sample_rate: pcm.sample_rate,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::sine;

    fn rms_db(p: &Pcm) -> f64 {
        let e: f64 =
            p.samples.iter().map(|&s| (s as f64).powi(2)).sum::<f64>() / p.samples.len() as f64;
        10.0 * e.log10()
    }

    #[test]
    fn radio_effect_attenuates_low_and_high() {
        for rate in [16000, 22050, 48000] {
            let gain = |f: f64| {
                let p = sine(rate, f, 1.0, 0.3);
                let out = radio_effect(&p, 1);
                // skip filter transient
                let tail = Pcm {
                    sample_rate: rate,
                    samples: out.samples[2000..].to_vec(),
                };
                let tin = Pcm {
                    sample_rate: rate,
                    samples: p.samples[2000..].to_vec(),
                };
                rms_db(&tail) - rms_db(&tin)
            };
            assert!(gain(100.0) <= -12.0, "rate {rate} 100Hz: {}", gain(100.0));
            if rate >= 16000 {
                assert!(gain(6000.0) <= -12.0, "rate {rate} 6kHz: {}", gain(6000.0));
            }
            assert!(gain(1000.0) >= -3.0, "rate {rate} 1kHz: {}", gain(1000.0));
        }
    }

    #[test]
    fn radio_effect_deterministic_for_seed() {
        let p = sine(16000, 800.0, 0.2, 0.4);
        assert_eq!(radio_effect(&p, 7), radio_effect(&p, 7));
        assert_ne!(radio_effect(&p, 7), radio_effect(&p, 8));
    }

    #[test]
    fn silence_gets_noise_near_minus_30_dbfs() {
        let p = Pcm {
            sample_rate: 16000,
            samples: vec![0; 16000],
        };
        let db = rms_db(&radio_effect(&p, 3)) - 90.31; // 20*log10(32768)
        assert!((db + 30.0).abs() < 1.5, "{db}");
    }
}
