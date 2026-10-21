use crate::filter::{Coefficients, FilterKind, Svf};
use nice_plug::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct DynamicControl {
    pub enabled: bool,
    pub threshold: f32,
    pub range: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
}

impl Default for DynamicControl {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: -24.0,
            range: -6.0,
            attack_ms: 10.0,
            release_ms: 100.0,
        }
    }
}

#[derive(Params)]
pub struct DynamicParams {
    #[id = "dynamic_enabled"]
    pub enabled: BoolParam,
    #[id = "dynamic_threshold"]
    pub threshold: FloatParam,
    #[id = "dynamic_range"]
    pub range: FloatParam,
    #[id = "dynamic_attack"]
    pub attack: FloatParam,
    #[id = "dynamic_release"]
    pub release: FloatParam,
}

impl DynamicParams {
    pub fn new(number: usize, prefix: &str) -> Self {
        let name = |label| format!("{prefix}Band {number} dynamic {label}");
        let time = |label, default, max| {
            FloatParam::new(
                name(label),
                default,
                FloatRange::Skewed {
                    min: 0.1,
                    max,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms")
            .with_value_to_string(formatters::v2s_f32_rounded(2))
        };

        Self {
            enabled: BoolParam::new(name("enabled"), false),
            threshold: FloatParam::new(
                name("threshold"),
                -24.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: 0.0,
                },
            )
            .with_unit(" dBFS")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            range: FloatParam::new(
                name("range"),
                -6.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            attack: time("attack", 10.0, 500.0),
            release: time("release", 100.0, 2000.0),
        }
    }

    pub fn control(&self) -> DynamicControl {
        DynamicControl {
            enabled: self.enabled.value(),
            threshold: self.threshold.value(),
            range: self.range.value(),
            attack_ms: self.attack.value(),
            release_ms: self.release.value(),
        }
    }
}

pub struct Dynamics {
    filters: [Svf; 2],
    power: f32,
    power_alpha: f32,
    times: [f32; 2],
    alphas: [f32; 2],
    pub gain: f32,
}

impl Dynamics {
    pub fn reset(&mut self) {
        self.filters = [Svf::default(); 2];
        self.power = 0.0;
        self.gain = 0.0;
    }

    pub fn new(rate: f32) -> Self {
        Self {
            filters: [Svf::default(); 2],
            power: 0.0,
            power_alpha: 1.0 - (-1.0 / (0.005 * rate)).exp(),
            times: [10.0, 100.0],
            alphas: [
                1.0 - (-1.0 / (0.010 * rate)).exp(),
                1.0 - (-1.0 / (0.100 * rate)).exp(),
            ],
            gain: 0.0,
        }
    }

    pub fn process(
        &mut self,
        source: [f32; 2],
        control: DynamicControl,
        rate: f32,
        frequency: f32,
        q: f32,
        kind: FilterKind,
    ) -> f32 {
        for (i, time) in [control.attack_ms, control.release_ms]
            .into_iter()
            .enumerate()
        {
            if self.times[i] != time {
                self.times[i] = time;
                self.alphas[i] = 1.0 - (-1.0 / (time * 0.001 * rate)).exp();
            }
        }

        let mut target = 0.0;
        if control.enabled && kind.has_gain() {
            let coefficients = match kind {
                FilterKind::Peak => {
                    let c = Coefficients::lowpass(rate, frequency, q);
                    // Unity gain at the tracked center, independent of Q.
                    Coefficients::new(c.g, c.k, [0.0, c.k, 0.0])
                }
                FilterKind::LowShelf => {
                    Coefficients::lowpass(rate, frequency, std::f32::consts::FRAC_1_SQRT_2)
                }
                FilterKind::HighShelf => Coefficients::for_kind(
                    rate,
                    frequency,
                    std::f32::consts::FRAC_1_SQRT_2,
                    0.0,
                    FilterKind::Highpass,
                ),
                _ => unreachable!(),
            };
            let filtered: [f32; 2] =
                std::array::from_fn(|ch| self.filters[ch].process(source[ch], coefficients));
            self.power += self.power_alpha
                * ((filtered[0] * filtered[0] + filtered[1] * filtered[1]) * 0.5 - self.power);
            if self.power > 0.0 {
                // Fixed 4:1 downward compression, bounded by the user's Range.
                target = (-(10.0 * self.power.log10() - control.threshold).max(0.0) * 0.75)
                    .max(control.range);
            }
        } else {
            self.filters = [Svf::default(); 2];
            self.power = 0.0;
        }

        let alpha = self.alphas[usize::from(target > self.gain)];
        self.gain += alpha * (target - self.gain);
        self.gain
    }
}
