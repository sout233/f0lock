use nice_plug::prelude::*;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum OperatingMode {
    #[default]
    #[id = "track_eq"]
    #[name = "Track EQ"]
    Track,
    #[id = "sidechain_eq"]
    #[name = "Sidechain EQ"]
    Sidechain,
}

impl OperatingMode {
    pub const ALL: [Self; 2] = [Self::Track, Self::Sidechain];

    pub fn label(self) -> &'static str {
        match self {
            Self::Track => "Track EQ",
            Self::Sidechain => "Sidechain EQ",
        }
    }
}
