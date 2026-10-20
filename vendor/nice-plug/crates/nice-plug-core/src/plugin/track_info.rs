//! Information about the track a plugin is placed on, as reported by the host.

/// Information about the track the plugin is currently placed on. Not all hosts provide this
/// information, and not all fields may be available at once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TrackInfo {
    name: String,
    color: Option<TrackColor>,
}

impl TrackInfo {
    pub fn new(name: impl Into<String>, color: Option<TrackColor>) -> Self {
        Self {
            name: name.into(),
            color,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn color(&self) -> Option<TrackColor> {
        self.color
    }
}

/// An RGBA color associated with a track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackColor {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl TrackColor {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgba(self) -> (u8, u8, u8, u8) {
        (self.r, self.g, self.b, self.a)
    }
}
