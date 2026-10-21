use super::{MAX_HZ, MIN_HZ, candidates::Observations};
use nice_plug::prelude::*;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TargetMode {
    #[default]
    #[id = "auto"]
    Auto,
    #[id = "range"]
    Range,
    #[id = "reference"]
    Reference,
}

impl TargetMode {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Range, Self::Reference];

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Range => "Range",
            Self::Reference => "Reference",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    pub mode: TargetMode,
    pub minimum: f32,
    pub maximum: f32,
    pub reference: f32,
    pub tolerance: f32,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            mode: TargetMode::Auto,
            minimum: 100.0,
            maximum: 600.0,
            reference: 300.0,
            tolerance: 6.0,
        }
    }
}

impl Target {
    pub fn bounds(self) -> (f32, f32) {
        match self.mode {
            TargetMode::Auto => (MIN_HZ, MAX_HZ),
            TargetMode::Range => (self.minimum, self.maximum),
            TargetMode::Reference => {
                let ratio = 2.0_f32.powf(self.tolerance / 12.0);
                (
                    (self.reference / ratio).max(MIN_HZ),
                    (self.reference * ratio).min(MAX_HZ),
                )
            }
        }
    }

    pub fn allows(self, hz: f32) -> bool {
        let (minimum, maximum) = self.bounds();
        hz >= minimum && hz <= maximum
    }

    pub(super) fn apply_preference(self, observations: &mut Observations) {
        if self.mode != TargetMode::Reference {
            return;
        }

        let original: f32 = observations.probability.iter().sum();

        for (probability, hz) in observations
            .probability
            .iter_mut()
            .zip(observations.frequency)
        {
            if *probability > 0.0 {
                let distance = 12.0 * (hz / self.reference).log2() / self.tolerance;
                *probability *= (-2.0 * distance * distance).exp();
            }
        }

        let weighted: f32 = observations.probability.iter().sum();

        if weighted > 0.0 {
            // Reference changes relative pitch preference, not the evidence
            // that a voiced signal exists. Never manufacture voicing mass.
            for probability in &mut observations.probability {
                *probability *= original / weighted;
            }
        }
    }
}
