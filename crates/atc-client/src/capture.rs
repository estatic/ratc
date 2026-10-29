//! Microphone samples (any rate) → 20 ms frames of 16 kHz mono PCM for the server.

/// Pilot audio rate expected by the server.
pub const PILOT_RATE: u32 = 16_000;
/// 20 ms at [`PILOT_RATE`].
pub const FRAME_LEN: usize = (PILOT_RATE / 50) as usize;

/// Streaming linear-interpolation resampler plus framer. Unlike resampling each callback
/// buffer on its own, the fractional read position carries over between pushes, so the
/// output has no seams and no drift whatever the device's buffer sizes are.
pub struct Framer {
    /// Input samples per output sample.
    step: f64,
    /// Read position (in input samples) of the next output sample within `input`.
    pos: f64,
    input: Vec<i16>,
    out: Vec<i16>,
}

impl Framer {
    pub fn new(in_rate: u32) -> Self {
        Framer {
            step: f64::from(in_rate) / f64::from(PILOT_RATE),
            pos: 0.0,
            input: Vec::new(),
            out: Vec::with_capacity(FRAME_LEN),
        }
    }

    /// Adds mono samples at the input rate; returns the frames completed by them.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Vec<i16>> {
        self.input.extend_from_slice(samples);
        let mut frames = Vec::new();
        while self.pos + 1.0 < self.input.len() as f64 {
            let i = self.pos as usize;
            let frac = self.pos - i as f64;
            let (a, b) = (f64::from(self.input[i]), f64::from(self.input[i + 1]));
            self.out.push((a + (b - a) * frac).round() as i16);
            self.pos += self.step;
            if self.out.len() == FRAME_LEN {
                frames.push(std::mem::replace(
                    &mut self.out,
                    Vec::with_capacity(FRAME_LEN),
                ));
            }
        }
        let used = (self.pos as usize).min(self.input.len());
        self.input.drain(..used);
        self.pos -= used as f64;
        frames
    }
}

/// Averages interleaved channels to mono.
pub fn downmix(interleaved: &[f32], channels: usize) -> Vec<i16> {
    interleaved
        .chunks(channels.max(1))
        .map(|c| {
            let m = c.iter().sum::<f32>() / c.len() as f32;
            (m.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_buffer_sizes_give_exact_frame_count() {
        for rate in [48_000, 44_100, 16_000, 22_050] {
            let mut f = Framer::new(rate);
            let input: Vec<i16> = (0..rate).map(|i| (i % 1000) as i16).collect();
            let mut frames = 0;
            for chunk in input.chunks(437) {
                let got = f.push(chunk);
                assert!(got.iter().all(|fr| fr.len() == FRAME_LEN));
                frames += got.len();
            }
            // one second of input is 50 frames (the last one may still wait for its tail)
            assert!((49..=50).contains(&frames), "{rate}: {frames}");
        }
    }

    #[test]
    fn continuous_ramp_has_no_seams() {
        // a slow ramp at 48 kHz stays a ramp at 16 kHz across push boundaries
        let mut f = Framer::new(48_000);
        let input: Vec<i16> = (0..9600).map(|i| (i / 3) as i16).collect();
        let mut out = Vec::new();
        for chunk in input.chunks(100) {
            out.extend(f.push(chunk).into_iter().flatten());
        }
        assert_eq!(out.len(), 10 * FRAME_LEN);
        assert!(out.windows(2).all(|w| w[1] - w[0] == 1), "{:?}", &out[..20]);
    }

    #[test]
    fn downmix_stereo() {
        assert_eq!(downmix(&[1.0, 0.0, -0.5, -0.5], 2), [16384, -16384]);
        assert_eq!(downmix(&[2.0], 1), [i16::MAX]);
    }
}
