mod candidates;
mod melody;
mod target;
mod tracker;

pub use target::{Target, TargetMode};

#[cfg(test)]
mod tests;

use candidates::{Observations, SpectralEvidence, mpm_observations, pyin_observations};
use nice_plug::prelude::*;
use pitch_estimate::{McLeodDetector, YinDetector};
use tracker::Tracker;

pub const MIN_HZ: f32 = 50.0;
pub const MAX_HZ: f32 = 2000.0;
const PERIODIC_MAX_HZ: f32 = 1000.0;
const PERIODIC_BINS: usize = 520;
// Ten-cent bins, including the nearest bin for the upper endpoint.
const BINS: usize = 640;
const UNVOICED: usize = BINS;
const STATES: usize = BINS * 2;
pub const LOOKAHEAD: usize = 3;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DetectionMode {
    #[id = "mpm"]
    #[name = "MPM"]
    Mpm,

    #[id = "mpm_tracking"]
    #[name = "MPM + Tracking"]
    MpmTracking,

    #[id = "pyin"]
    #[name = "pYIN"]
    Pyin,

    #[default]
    #[id = "spectral_melody"]
    #[name = "F0LOCK Ψ"]
    SpectralMelody,
}

impl DetectionMode {
    pub const ALL: [Self; 4] = [
        Self::Mpm,
        Self::MpmTracking,
        Self::Pyin,
        Self::SpectralMelody,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mpm => "MPM",
            Self::MpmTracking => "MPM + Tracking",
            Self::Pyin => "pYIN",
            Self::SpectralMelody => "F0LOCK Ψ",
        }
    }
}

pub struct PitchTracking {
    pub decision_ready: bool,
    mode: DetectionMode,
    target: Target,
    yin: YinDetector<f32>,
    evidence: SpectralEvidence,
    melody: melody::MelodyEvidence,
    observations: Observations,
    tracker: Tracker,
    melody_tracker: Tracker<BINS>,
}

impl PitchTracking {
    pub fn new(rate: f32, window: usize, hop: usize) -> Self {
        Self {
            decision_ready: false,
            mode: DetectionMode::default(),
            target: Target::default(),
            yin: YinDetector::new(window, window / 2).expect("Invalid pYIN window"),
            evidence: SpectralEvidence::new(window),
            melody: melody::MelodyEvidence::new(window),
            observations: Observations::default(),
            tracker: Tracker::new(hop as f32 / rate),
            melody_tracker: Tracker::new(hop as f32 / rate),
        }
    }

    pub fn reset(&mut self) {
        self.decision_ready = false;
        self.tracker.reset();
        self.melody_tracker.reset();
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
            DetectionMode::SpectralMelody => {
                self.melody
                    .analyze(frame, rate, target, &mut self.observations);
            }
        }

        target.apply_preference(&mut self.observations);

        if self.mode == DetectionMode::Mpm {
            self.decision_ready = true;
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

        let lag = if matches!(
            self.mode,
            DetectionMode::Pyin | DetectionMode::SpectralMelody
        ) {
            LOOKAHEAD
        } else {
            0
        };
        let (result, ready) = if self.mode == DetectionMode::SpectralMelody {
            (
                self.melody_tracker
                    .process(&self.observations, threshold, lag),
                self.melody_tracker.frames > lag,
            )
        } else {
            (
                self.tracker.process(&self.observations, threshold, lag),
                self.tracker.frames > lag,
            )
        };
        let (frequency, clarity) = result;
        self.decision_ready = ready;
        // A delayed decision may predate an automated range change.
        // Keep its trajectory, but never emit a now-excluded pending pitch.
        if target.mode != TargetMode::Auto && !target.allows(frequency) {
            (0.0, clarity)
        } else {
            (frequency, clarity)
        }
    }
}
