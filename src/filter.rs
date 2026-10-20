use nice_plug::prelude::*;
use realfft::num_complex::Complex32;
use std::f32::consts::PI;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    #[default]
    #[id = "peak"]
    Peak,
    #[id = "low_shelf"]
    #[name = "Low Shelf"]
    LowShelf,
    #[id = "high_shelf"]
    #[name = "High Shelf"]
    HighShelf,
    #[id = "highpass"]
    #[name = "High Pass"]
    Highpass,
    #[id = "lowpass"]
    #[name = "Low Pass"]
    Lowpass,
    #[id = "notch"]
    Notch,
}

impl FilterKind {
    pub const ALL: [Self; 6] = [
        Self::Peak,
        Self::LowShelf,
        Self::HighShelf,
        Self::Highpass,
        Self::Lowpass,
        Self::Notch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Peak => "Peak",
            Self::LowShelf => "Low Shelf",
            Self::HighShelf => "High Shelf",
            Self::Highpass => "High Pass",
            Self::Lowpass => "Low Pass",
            Self::Notch => "Notch",
        }
    }

    pub fn has_gain(self) -> bool {
        matches!(self, Self::Peak | Self::LowShelf | Self::HighShelf)
    }
}

#[derive(Default, Clone, Copy)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Coefficients {
    a1: f32,
    a2: f32,
    a3: f32,
    pub g: f32,
    pub k: f32,
    pub mix: [f32; 3],
}

impl Coefficients {
    pub fn new(g: f32, k: f32, mix: [f32; 3]) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));

        Self {
            a1,
            a2: g * a1,
            a3: g * g * a1,
            g,
            k,
            mix,
        }
    }

    pub fn lowpass(rate: f32, frequency: f32, q: f32) -> Self {
        Self::new((PI * frequency / rate).tan(), 1.0 / q, [0.0, 0.0, 1.0])
    }

    pub fn bell(rate: f32, frequency: f32, q: f32, db: f32) -> Self {
        Self::for_kind(rate, frequency, q, db, FilterKind::Peak)
    }

    /// Cytomic TPT SVF output mixes; shelf prewarping keeps the midpoint at fc.
    pub fn for_kind(rate: f32, frequency: f32, q: f32, db: f32, kind: FilterKind) -> Self {
        let a = 10.0_f32.powf(db / 40.0);
        let mut g = (PI * frequency / rate).tan();
        let mut k = 1.0 / q;

        let mix = match kind {
            FilterKind::Peak => {
                k /= a;
                [1.0, k * (a * a - 1.0), 0.0]
            }
            FilterKind::LowShelf => {
                g /= a.sqrt();
                [1.0, k * (a - 1.0), a * a - 1.0]
            }
            FilterKind::HighShelf => {
                g *= a.sqrt();
                [a * a, k * (1.0 - a) * a, 1.0 - a * a]
            }
            FilterKind::Highpass => [1.0, -k, -1.0],
            FilterKind::Lowpass => [0.0, 0.0, 1.0],
            FilterKind::Notch => [1.0, -k, 0.0],
        };

        Self::new(g, k, mix)
    }

    pub fn approach(self, target: Self, alpha: f32) -> Self {
        Self::new(
            self.g + alpha * (target.g - self.g),
            self.k + alpha * (target.k - self.k),
            std::array::from_fn(|i| self.mix[i] + alpha * (target.mix[i] - self.mix[i])),
        )
    }

    pub fn response(self, rate: f32, frequency: f32) -> Complex32 {
        let r = (PI * frequency / rate).tan() / self.g;
        let denominator = Complex32::new(1.0 - r * r, self.k * r);

        Complex32::new(self.mix[0], 0.0)
            + (Complex32::new(self.mix[2], self.mix[1] * r) / denominator)
    }
}

impl Svf {
    pub fn process(&mut self, input: f32, c: Coefficients) -> f32 {
        let (band, low) = self.step(input, c);

        c.mix[0] * input + c.mix[1] * band + c.mix[2] * low
    }

    #[cfg(test)]
    pub fn bell(&mut self, input: f32, c: Coefficients) -> f32 {
        self.process(input, c)
    }

    pub fn lowpass(&mut self, input: f32, c: Coefficients) -> f32 {
        self.step(input, c).1
    }

    fn step(&mut self, input: f32, c: Coefficients) -> (f32, f32) {
        let v3 = input - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;

        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;

        (v1, v2)
    }
}

#[cfg(test)]
pub fn response_db(rate: f32, frequency: f32, center: f32, q: f32, db: f32) -> f32 {
    20.0 * Coefficients::bell(rate, center, q, db)
        .response(rate, frequency)
        .norm()
        .log10()
}
