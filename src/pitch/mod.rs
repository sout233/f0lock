mod candidates;
mod tracker;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod sustain_tests;

use candidates::{Observations, SpectralEvidence, mpm_observations, pyin_observations};
use nice_plug::prelude::*;
use pitch_estimate::{McLeodDetector, YinDetector};
use tracker::Tracker;

pub const MIN_HZ: f32 = 50.0;
pub const MAX_HZ: f32 = 1000.0;
// Ten-cent bins cover the same range as the original detector.
const BINS: usize = 520;
const UNVOICED: usize = BINS;
const STATES: usize = BINS * 2;
pub const LOOKAHEAD: usize = 3;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMode {
    #[default]
    #[id = "mpm"]
    #[name = "MPM"]
    Mpm,

    #[id = "mpm_tracking"]
    #[name = "MPM + Tracking"]
    MpmTracking,

    #[id = "pyin"]
    #[name = "pYIN"]
    Pyin,
}

impl DetectionMode {
    pub const ALL: [Self; 3] = [Self::Mpm, Self::MpmTracking, Self::Pyin];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mpm => "MPM",
            Self::MpmTracking => "MPM + Tracking",
            Self::Pyin => "pYIN",
        }
    }
}

pub struct PitchTracking {
    mode: DetectionMode,
    yin: YinDetector<f32>,
    evidence: SpectralEvidence,
    observations: Observations,
    tracker: Tracker,
}

impl PitchTracking {
    pub fn new(rate: f32, window: usize, hop: usize) -> Self {
        Self {
            mode: DetectionMode::Mpm,
            yin: YinDetector::new(window, window / 2).expect("Invalid pYIN window"),
            evidence: SpectralEvidence::new(window),
            observations: Observations::default(),
            tracker: Tracker::new(hop as f32 / rate),
        }
    }

    pub fn reset(&mut self) {
        self.tracker.reset();
    }

    pub fn set_mode(&mut self, mode: DetectionMode) {
        if mode != self.mode {
            self.mode = mode;
            self.reset();
        }
    }

    pub fn analyze(
        &mut self,
        frame: &[f32],
        rate: f32,
        mpm: &mut McLeodDetector<f32>,
        threshold: f32,
    ) -> (f32, f32) {
        self.observations.clear();

        match self.mode {
            DetectionMode::MpmTracking => {
                self.evidence.analyze(frame);
                mpm_observations(mpm, frame, rate, &self.evidence, &mut self.observations);
            }
            DetectionMode::Pyin => {
                if let Some((difference, curve)) = self.yin.analyze_curves(frame) {
                    pyin_observations(curve, difference, rate, &mut self.observations);
                }
            }
            DetectionMode::Mpm => unreachable!("Raw MPM uses its original detection path"),
        }

        let lag = if self.mode == DetectionMode::Pyin {
            LOOKAHEAD
        } else {
            0
        };
        self.tracker.process(&self.observations, threshold, lag)
    }
}
