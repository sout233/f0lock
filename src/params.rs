use crate::{
    band::{BAND_COUNT, BandControl},
    filter::FilterKind,
};
use nice_plug::prelude::*;
use std::sync::atomic::AtomicU32;

pub const UI_SCALES: [u32; 8] = [75, 90, 100, 110, 125, 150, 175, 200];

#[derive(Params)]
pub struct F0Params {
    #[persist = "ui_scale_percent"]
    pub ui_scale_percent: AtomicU32,

    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "q"]
    pub q: FloatParam,

    #[id = "tracking"]
    pub tracking: FloatParam,

    #[id = "clarity"]
    pub clarity: FloatParam,

    #[id = "gate"]
    pub gate: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[id = "output"]
    pub output: FloatParam,

    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "f0_exists"]
    pub exists: BoolParam,

    #[id = "f0_enabled"]
    pub enabled: BoolParam,

    #[id = "f0_ratio"]
    pub ratio: FloatParam,

    #[id = "f0_type"]
    pub kind: EnumParam<FilterKind>,

    #[nested(array, group = "Additional band")]
    pub additional: [BandParams; BAND_COUNT - 1],
}

impl Default for F0Params {
    fn default() -> Self {
        Self {
            ui_scale_percent: AtomicU32::new(100),

            gain: FloatParam::new(
                "F0 gain",
                -6.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            q: FloatParam::new(
                "Q",
                3.0,
                FloatRange::Skewed {
                    min: 0.3,
                    max: 16.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            tracking: FloatParam::new(
                "Tracking",
                35.0,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms"),

            clarity: FloatParam::new(
                "Clarity",
                0.85,
                FloatRange::Linear {
                    min: 0.5,
                    max: 0.99,
                },
            ),

            gate: FloatParam::new(
                "Detector gate",
                -55.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: -20.0,
                },
            )
            .with_unit(" dBFS"),

            mix: FloatParam::new(
                "Mix",
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" %"),

            output: FloatParam::new(
                "Output",
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            bypass: BoolParam::new("Bypass", false).make_bypass(),

            exists: BoolParam::new("F0 exists", true),
            enabled: BoolParam::new("F0 enabled", true),
            ratio: ratio_param("F0 ratio"),
            kind: EnumParam::new("F0 type", FilterKind::Peak),

            additional: std::array::from_fn(|i| BandParams::new(i + 2)),
        }
    }
}

#[derive(Params)]
pub struct BandParams {
    #[id = "band_gain"]
    pub gain: FloatParam,

    #[id = "band_q"]
    pub q: FloatParam,

    #[id = "band_ratio"]
    pub ratio: FloatParam,

    #[id = "band_type"]
    pub kind: EnumParam<FilterKind>,

    #[id = "band_exists"]
    pub exists: BoolParam,

    #[id = "band_enabled"]
    pub enabled: BoolParam,
}

fn ratio_param(name: impl Into<String>) -> FloatParam {
    FloatParam::new(
        name,
        1.0,
        FloatRange::Skewed {
            min: 0.02,
            max: 400.0,
            factor: FloatRange::skew_factor(-2.0),
        },
    )
    .with_value_to_string(formatters::v2s_f32_rounded(3))
}

impl BandParams {
    fn new(number: usize) -> Self {
        Self {
            gain: FloatParam::new(
                format!("Band {number} gain"),
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            q: FloatParam::new(
                format!("Band {number} Q"),
                3.0,
                FloatRange::Skewed {
                    min: 0.3,
                    max: 16.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            ratio: ratio_param(format!("Band {number} ratio")),
            kind: EnumParam::new(format!("Band {number} type"), FilterKind::Peak),

            exists: BoolParam::new(format!("Band {number} exists"), false),
            enabled: BoolParam::new(format!("Band {number} enabled"), true),
        }
    }
}

#[derive(Clone, Copy)]
pub struct BandRef<'a> {
    pub gain: &'a FloatParam,
    pub q: &'a FloatParam,
    pub ratio: &'a FloatParam,
    pub kind: &'a EnumParam<FilterKind>,
    pub exists: &'a BoolParam,
    pub enabled: &'a BoolParam,
}

impl BandRef<'_> {
    pub fn control(self) -> BandControl {
        BandControl {
            gain: self.gain.value(),
            q: self.q.value(),
            ratio: self.ratio.value(),
            kind: self.kind.value(),
            exists: self.exists.value(),
            enabled: self.enabled.value(),
        }
    }
}

impl F0Params {
    pub fn band(&self, index: usize) -> BandRef<'_> {
        if index == 0 {
            BandRef {
                gain: &self.gain,
                q: &self.q,
                ratio: &self.ratio,
                kind: &self.kind,
                exists: &self.exists,
                enabled: &self.enabled,
            }
        } else {
            let b = &self.additional[index - 1];

            BandRef {
                gain: &b.gain,
                q: &b.q,
                ratio: &b.ratio,
                kind: &b.kind,
                exists: &b.exists,
                enabled: &b.enabled,
            }
        }
    }
}
