mod candidates;
mod target;
mod tracker;

pub use target::{Target, TargetMode};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod sustain_tests;

#[cfg(test)]
mod target_tests;

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
    target: Target,
    yin: YinDetector<f32>,
    evidence: SpectralEvidence,
    observations: Observations,
    tracker: Tracker,
}

impl PitchTracking {
    pub fn new(rate: f32, window: usize, hop: usize) -> Self {
        Self {
            mode: DetectionMode::Mpm,
            target: Target::default(),
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
        target: Target,
    ) -> (f32, f32) {
        if target.mode != self.target.mode {
            self.reset();
        }
        self.target = target;
        self.observations.clear();

        match self.mode {
            DetectionMode::Mpm | DetectionMode::MpmTracking => {
                self.evidence.analyze(frame);
                mpm_observations(
                    mpm,
                    frame,
                    rate,
                    &self.evidence,
                    &mut self.observations,
                    target,
                );
            }
            DetectionMode::Pyin => {
                if let Some((difference, curve)) = self.yin.analyze_curves(frame) {
                    pyin_observations(curve, difference, rate, &mut self.observations, target);
                }
            }
        }

        target.apply_preference(&mut self.observations);

        if self.mode == DetectionMode::Mpm {
            let best = (0..BINS)
                .filter(|&i| {
                    self.observations.probability[i] > 0.0
                        && self.observations.clarity[i] >= threshold
                })
                .max_by(|&a, &b| {
                    self.observations.probability[a].total_cmp(&self.observations.probability[b])
                });
            return best.map_or((0.0, 0.0), |i| {
                (self.observations.frequency[i], self.observations.clarity[i])
            });
        }

        let lag = if self.mode == DetectionMode::Pyin {
            LOOKAHEAD
        } else {
            0
        };
        let (frequency, clarity) = self.tracker.process(&self.observations, threshold, lag);
        // pYIN's delayed decision may predate an automated range change.
        // Keep its trajectory, but never emit a now-excluded pending pitch.
        if target.mode != TargetMode::Auto && !target.allows(frequency) {
            (0.0, clarity)
        } else {
            (frequency, clarity)
        }
    }
}
