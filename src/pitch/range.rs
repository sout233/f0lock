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
    #[id = "bass"]
    #[name = "Bass: 20–300 Hz"]
    Bass,
}

impl DetectionRange {
    pub const ALL: [Self; 3] = [Self::Standard, Self::Low, Self::Bass];

    pub fn minimum(self) -> f32 {
        match self {
            Self::Standard => super::MIN_HZ,
            Self::Low => super::EXTENDED_MIN_HZ,
            Self::Bass => super::BASS_MIN_HZ,
        }
    }

    pub fn maximum(self) -> f32 {
        match self {
            Self::Bass => super::BASS_MAX_HZ,
            Self::Standard | Self::Low => super::MAX_HZ,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "Standard: 50–2000 Hz",
            Self::Low => "Low: 25–2000 Hz",
            Self::Bass => "Bass: 20–300 Hz",
        }
    }
}
