use crate::dynamics::{DynamicControl, Dynamics};
use crate::filter::{Coefficients, FilterKind, Svf};

pub const BAND_COUNT: usize = 8;
pub const BAND_VALUES: usize = 10;

#[derive(Clone, Copy, Debug)]
pub struct BandControl {
    pub dynamics: DynamicControl,
    pub exists: bool,
    pub enabled: bool,
    pub ratio: f32,
    pub gain: f32,
    pub q: f32,
    pub kind: FilterKind,
}

impl Default for BandControl {
    fn default() -> Self {
        Self {
            dynamics: DynamicControl::default(),
            exists: false,
            enabled: true,
            ratio: 1.0,
            gain: 0.0,
            q: 3.0,
            kind: FilterKind::Peak,
        }
    }
}

pub struct Band {
    dynamics: Dynamics,
    filters: [Svf; 2],
    pub coefficients: Coefficients,
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
    pub wet: f32,
    log_ratio: f64,
}

impl Band {
    pub fn new(rate: f32) -> Self {
        Self {
            dynamics: Dynamics::new(rate),
            filters: [Svf::default(); 2],
            coefficients: Coefficients::bell(rate, 150.0, 3.0, 0.0),
            frequency: 150.0,
            gain: 0.0,
            q: 3.0,
            wet: 0.0,
            log_ratio: 0.0,
        }
    }

    pub fn process(
        &mut self,
        mut input: [f32; 2],
        source: [f32; 2],
        control: BandControl,
        f0: f32,
        voiced: bool,
        rate: f32,
        alpha: f32,
        envelope_steps: [f32; 2],
    ) -> [f32; 2] {
        let active = control.exists && control.enabled && voiced;

        if self.wet == 0.0 && !active {
            self.gain = 0.0;
            self.dynamics.reset();
            return input;
        }

        let target_frequency = (f0 * control.ratio).min(rate * 0.49);
        let dynamic_gain = self.dynamics.process(
            source,
            control.dynamics,
            rate,
            target_frequency,
            control.q,
            control.kind,
        );
        let target_gain = control.gain + dynamic_gain;

        if self.wet == 0.0 {
            self.log_ratio = f64::from(control.ratio).ln();
            self.frequency = target_frequency;
            self.gain = target_gain;
            self.q = control.q;
            self.coefficients =
                Coefficients::for_kind(rate, self.frequency, self.q, self.gain, control.kind);
        }

        self.log_ratio += f64::from(alpha) * (f64::from(control.ratio).ln() - self.log_ratio);
        self.frequency = (f0 * self.log_ratio.exp() as f32).min(rate * 0.49);
        self.gain += alpha * (target_gain - self.gain);
        self.q += alpha * (control.q - self.q);

        let target = Coefficients::for_kind(rate, self.frequency, self.q, self.gain, control.kind);

        // Smooth the poles AND output taps, retaining integrator states across type changes.
        self.coefficients = self.coefficients.approach(target, alpha);

        // A full 0-to-1 (or 1-to-0) ramp takes the specified time. Retrigger
        // from the current amount, preserving filter states until fully dry.
        let step = envelope_steps[usize::from(!active)];
        self.wet += (f32::from(active) - self.wet).clamp(-step, step);

        for (ch, sample) in input.iter_mut().enumerate() {
            let filtered = self.filters[ch].process(*sample, self.coefficients);
            *sample += self.wet * (filtered - *sample);
        }

        if self.wet == 0.0 {
            self.filters = [Svf::default(); 2];
        }
        input
    }

    pub fn values(&self) -> [f32; BAND_VALUES] {
        [
            self.frequency,
            self.gain,
            self.q,
            self.wet,
            self.coefficients.g,
            self.coefficients.k,
            self.coefficients.mix[0],
            self.coefficients.mix[1],
            self.coefficients.mix[2],
            self.dynamics.gain,
        ]
    }
}
