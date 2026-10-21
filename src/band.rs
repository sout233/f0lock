use crate::dynamics::{DynamicControl, Dynamics};
use crate::filter::{Coefficients, CutSlope, FilterKind, Svf};

pub const BAND_COUNT: usize = 8;
pub const CUT_SECTIONS: usize = 8;
pub const BAND_VALUES: usize = 10 + (CUT_SECTIONS - 1) * 6;

#[derive(Clone, Copy, Debug)]
pub struct BandControl {
    pub dynamics: DynamicControl,
    pub exists: bool,
    pub enabled: bool,
    pub ratio: f32,
    pub gain: f32,
    pub q: f32,
    pub kind: FilterKind,
    pub slope: CutSlope,
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
            slope: CutSlope::default(),
        }
    }
}

pub struct Band {
    dynamics: Dynamics,
    filters: [[Svf; 2]; CUT_SECTIONS],
    extra_coefficients: [Coefficients; CUT_SECTIONS - 1],
    extra_wet: [f32; CUT_SECTIONS - 1],
    slope: CutSlope,
    section_q: [f32; CUT_SECTIONS],
    sections: usize,
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
            filters: [[Svf::default(); 2]; CUT_SECTIONS],
            extra_coefficients: [Coefficients::identity(); CUT_SECTIONS - 1],
            extra_wet: [0.0; CUT_SECTIONS - 1],
            slope: CutSlope::default(),
            section_q: CutSlope::default().section_q(),
            sections: 1,
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
        let section_count = if control.kind.is_cut() {
            control.slope.order().div_ceil(2)
        } else {
            1
        };
        if self.slope != control.slope {
            self.slope = control.slope;
            self.section_q = self.slope.section_q();
        }
        let first_order = control.slope.order() % 2 == 1;

        let starting = self.wet == 0.0;
        if starting {
            self.log_ratio = f64::from(control.ratio).ln();
            self.frequency = target_frequency;
            self.gain = target_gain;
            self.q = control.q;
        }

        self.log_ratio += f64::from(alpha) * (f64::from(control.ratio).ln() - self.log_ratio);
        self.frequency = (f0 * self.log_ratio.exp() as f32).min(rate * 0.49);
        self.gain += alpha * (target_gain - self.gain);
        self.q += alpha * (control.q - self.q);

        let target = if control.kind.is_cut() {
            Coefficients::cut_section(
                (std::f32::consts::PI * self.frequency / rate).tan(),
                self.q * self.section_q[0],
                control.kind,
                control.slope.order() == 1,
            )
        } else {
            Coefficients::for_kind(rate, self.frequency, self.q, self.gain, control.kind)
        };

        // Smooth the poles AND output taps, retaining integrator states across type changes.
        self.coefficients = if starting {
            target
        } else {
            self.coefficients.approach(target, alpha)
        };

        self.sections = self.sections.max(section_count);
        for i in 1..self.sections {
            let enabled = i < section_count;
            if enabled {
                let target = Coefficients::cut_section(
                    target.g,
                    self.section_q[i],
                    control.kind,
                    first_order && i == section_count - 1,
                );
                self.extra_coefficients[i - 1] = if self.extra_wet[i - 1] == 0.0 {
                    target
                } else {
                    self.extra_coefficients[i - 1].approach(target, alpha)
                };
            }
            let wet = &mut self.extra_wet[i - 1];
            if starting && enabled {
                *wet = 1.0;
            } else {
                *wet += (f32::from(enabled) - *wet).clamp(-alpha, alpha);
            }
            if *wet == 0.0 {
                self.filters[i] = [Svf::default(); 2];
            }
        }
        while self.sections > section_count && self.extra_wet[self.sections - 2] == 0.0 {
            self.sections -= 1;
        }

        // A full 0-to-1 (or 1-to-0) ramp takes the specified time. Retrigger
        // from the current amount, preserving filter states until fully dry.
        let step = envelope_steps[usize::from(!active)];
        self.wet += (f32::from(active) - self.wet).clamp(-step, step);

        for (ch, sample) in input.iter_mut().enumerate() {
            let mut filtered = self.filters[0][ch].process(*sample, self.coefficients);
            for i in 1..self.sections {
                if self.extra_wet[i - 1] > 0.0 {
                    let next =
                        self.filters[i][ch].process(filtered, self.extra_coefficients[i - 1]);
                    filtered += self.extra_wet[i - 1] * (next - filtered);
                }
            }
            *sample += self.wet * (filtered - *sample);
        }

        if self.wet == 0.0 {
            self.filters = [[Svf::default(); 2]; CUT_SECTIONS];
            self.extra_wet.fill(0.0);
            self.sections = 1;
        }
        input
    }

    pub fn values(&self) -> [f32; BAND_VALUES] {
        let mut values = [0.0; BAND_VALUES];
        values[..10].copy_from_slice(&[
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
        ]);
        for (i, c) in self.extra_coefficients.iter().enumerate() {
            let offset = 10 + i * 6;
            values[offset..offset + 6].copy_from_slice(&[
                c.g,
                c.k,
                c.mix[0],
                c.mix[1],
                c.mix[2],
                self.extra_wet[i],
            ]);
        }
        values
    }
}
