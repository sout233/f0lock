use nice_plug::prelude::*;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum DetectionRange {
    #[default]
    #[id = "standard"]
    #[name = "Standard: 50–2000 Hz"]
    Standard,
    #[id = "low"]
    #[name = "Low: 25–2000 Hz"]
    Low,
}

impl DetectionRange {
    pub const ALL: [Self; 2] = [Self::Standard, Self::Low];

    pub fn minimum(self) -> f32 {
        match self {
            Self::Standard => super::MIN_HZ,
            Self::Low => super::EXTENDED_MIN_HZ,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard: 50–2000 Hz",
            Self::Low => "Low: 25–2000 Hz",
        }
    }
}
