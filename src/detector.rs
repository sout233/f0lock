use crate::filter::{Coefficients, Svf};
use crate::pitch::{DetectionMode, PitchTracking, Target, TargetMode};
use pitch_estimate::{McLeodDetector, PitchDetector};

pub const WINDOW: usize = 1024;
pub const HOP: usize = 128;

#[derive(Clone, Copy, Default, Debug)]
pub struct Estimate {
    pub frequency: f32,
    pub clarity: f32,
    pub rms: f32,
}

pub struct Detector {
    pub mode: DetectionMode,
    pub target: Target,
    tracking: PitchTracking,
    pub rate: f32,
    decimation: usize,
    phase: usize,
    filters: [[Svf; 2]; 2],
    lowpass: [Coefficients; 2],
    rings: [[f32; WINDOW]; 2],
    position: usize,
    filled: usize,
    hop: usize,
    mpm: McLeodDetector<f32>,
    frame: [f32; WINDOW],
}

impl Detector {
    pub fn new(sample_rate: f32) -> Self {
        let decimation = (sample_rate / 12000.0).floor().max(1.0) as usize;
        let rate = sample_rate / decimation as f32;

        let mpm = McLeodDetector::new(WINDOW, (rate / 50.0 * 1.6).ceil() as usize)
            .expect("Invalid MPM window configuration")
            .with_k(0.93)
            .with_clarity_threshold(0.0);

        Self {
            mode: DetectionMode::Mpm,
            target: Target::default(),
            tracking: PitchTracking::new(rate, WINDOW, HOP),
            rate,
            decimation,
            phase: 0,
            filters: [[Svf::default(); 2]; 2],
            lowpass: [
                Coefficients::lowpass(sample_rate, 1000.0, 0.5411961),
                Coefficients::lowpass(sample_rate, 1000.0, 1.306563),
            ],
            rings: [[0.0; WINDOW]; 2],
            position: 0,
            filled: 0,
            hop: 0,
            mpm,
            frame: [0.0; WINDOW],
        }
    }

    pub fn reset(&mut self) {
        self.tracking.reset();
        self.filters = [[Svf::default(); 2]; 2];
        self.rings = [[0.0; WINDOW]; 2];
        self.position = 0;
        self.filled = 0;

        self.phase = 0;

        self.hop = 0;
    }

    pub fn push(&mut self, samples: [f32; 2]) -> bool {
        // Fourth-order Butterworth at 1 kHz before decimation. Independent
        // channels prevent cancellation of antiphase stereo material.
        let mut filtered = [0.0; 2];

        for ch in 0..2 {
            let first = self.filters[ch][0].lowpass(samples[ch], self.lowpass[0]);
            filtered[ch] = self.filters[ch][1].lowpass(first, self.lowpass[1]);
        }

        self.phase += 1;

        if self.phase != self.decimation {
            return false;
        }

        self.phase = 0;

        for (ch, value) in filtered.into_iter().enumerate() {
            self.rings[ch][self.position] = value;
        }

        self.position = (self.position + 1) % WINDOW;
        self.filled = (self.filled + 1).min(WINDOW);
        self.hop += 1;

        if self.hop < HOP {
            return false;
        }

        self.hop = 0;
        self.filled == WINDOW
    }

    pub fn analyze(&mut self, gate_db: f32, threshold: f32) -> Estimate {
        self.tracking.set_mode(self.mode);
        let means: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().sum::<f32>() / WINDOW as f32);
        let powers: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().map(|v| (v - means[ch]).powi(2)).sum());
        let channel = usize::from(powers[1] > powers[0]);
        let mean = means[channel];

        for i in 0..WINDOW {
            self.frame[i] = self.rings[channel][(self.position + i) % WINDOW] - mean;
        }

        let rms = (self.frame.iter().map(|v| v * v).sum::<f32>() / WINDOW as f32).sqrt();

        if rms < 10.0_f32.powf(gate_db / 20.0) {
            self.tracking.reset();
            return Estimate {
                rms,
                ..Estimate::default()
            };
        }

        if self.mode != DetectionMode::Mpm || self.target.mode != TargetMode::Auto {
            let (frequency, clarity) = self.tracking.analyze(
                &self.frame,
                self.rate,
                &mut self.mpm,
                threshold,
                self.target,
            );

            return Estimate {
                frequency,
                clarity,
                rms,
            };
        }

        match self.mpm.detect(&self.frame, self.rate.round() as u32) {
            Some(pitch) => {
                let hz = pitch.frequency * self.rate / self.rate.round();
                let valid = pitch.clarity >= threshold && (50.0..=1000.0).contains(&hz);
                Estimate {
                    frequency: if valid { hz } else { 0.0 },
                    clarity: pitch.clarity,
                    rms,
                }
            }
            None => Estimate {
                rms,
                ..Estimate::default()
            },
        }
    }
}
