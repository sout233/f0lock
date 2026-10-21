use crate::{
    band::{BAND_COUNT, BandControl},
    dynamics::DynamicParams,
    filter::FilterKind,
    lookahead::LookaheadTime,
    mode::OperatingMode,
    pitch::{DetectionMode, MAX_HZ, MIN_HZ, Target, TargetMode},
};
use nice_plug::prelude::*;
use std::sync::atomic::AtomicU32;

pub const UI_SCALES: [u32; 8] = [75, 90, 100, 110, 125, 150, 175, 200];

#[derive(Params)]
pub struct F0Params {
    #[id = "operating_mode"]
    pub mode: EnumParam<OperatingMode>,

    #[id = "bypass"]
    pub bypass: BoolParam,

    #[id = "lookahead"]
    pub lookahead: BoolParam,

    #[id = "lookahead_time"]
    pub lookahead_time: EnumParam<LookaheadTime>,

    #[persist = "ui_scale_percent"]
    pub ui_scale_percent: AtomicU32,

    #[nested(group = "Track EQ")]
    pub track: ProcessingParams,

    #[nested(id_prefix = "sc", group = "Sidechain EQ")]
    pub sidechain: ProcessingParams,
}

impl Default for F0Params {
    fn default() -> Self {
        Self {
            mode: EnumParam::new("Operating Mode", OperatingMode::Track),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
            lookahead: BoolParam::new("Lookahead", false).non_automatable(),
            lookahead_time: EnumParam::new("Lookahead Time", LookaheadTime::default())
                .non_automatable(),
            ui_scale_percent: AtomicU32::new(100),
            track: ProcessingParams::new(""),
            sidechain: ProcessingParams::new("Sidechain "),
        }
    }
}

impl F0Params {
    pub fn active(&self) -> &ProcessingParams {
        self.for_mode(self.mode.value())
    }

    pub fn for_mode(&self, mode: OperatingMode) -> &ProcessingParams {
        match mode {
            OperatingMode::Track => &self.track,
            OperatingMode::Sidechain => &self.sidechain,
        }
    }

    pub fn band(&self, index: usize) -> BandRef<'_> {
        self.active().band(index)
    }

    pub fn target(&self) -> Target {
        self.active().target()
    }
}

#[derive(Params)]
pub struct ProcessingParams {
    #[id = "pitch_detection"]
    pub detection: EnumParam<DetectionMode>,

    #[id = "target_mode"]
    pub target_mode: EnumParam<TargetMode>,

    #[id = "target_min"]
    pub target_min: FloatParam,

    #[id = "target_max"]
    pub target_max: FloatParam,

    #[id = "target_reference"]
    pub target_reference: FloatParam,

    #[id = "target_tolerance"]
    pub target_tolerance: FloatParam,

    #[id = "gain"]
    pub gain: FloatParam,

    #[id = "q"]
    pub q: FloatParam,

    #[id = "tracking"]
    pub tracking: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "clarity"]
    pub clarity: FloatParam,

    #[id = "gate"]
    pub gate: FloatParam,

    #[id = "mix"]
    pub mix: FloatParam,

    #[id = "output"]
    pub output: FloatParam,

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

    #[nested(array, group = "Dynamics")]
    pub dynamics: [DynamicParams; BAND_COUNT],
}

impl ProcessingParams {
    fn new(prefix: &str) -> Self {
        let name = |label| format!("{prefix}{label}");
        Self {
            detection: EnumParam::new(name("Pitch Detection"), DetectionMode::default()),
            dynamics: std::array::from_fn(|i| DynamicParams::new(i + 1, prefix)),
            target_mode: EnumParam::new(name("Target Mode"), TargetMode::Auto),
            target_min: target_frequency_param(&name("Target Minimum"), 100.0),
            target_max: target_frequency_param(&name("Target Maximum"), 600.0),
            target_reference: target_frequency_param(&name("Target Reference"), 300.0),
            target_tolerance: FloatParam::new(
                name("Target Tolerance"),
                6.0,
                FloatRange::Linear {
                    min: 1.0,
                    max: 24.0,
                },
            )
            .with_unit(" st")
            .with_value_to_string(formatters::v2s_f32_rounded(2)),
            gain: FloatParam::new(
                name("F0 gain"),
                -6.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            q: FloatParam::new(
                name("Q"),
                3.0,
                FloatRange::Skewed {
                    min: 0.3,
                    max: 16.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            tracking: FloatParam::new(
                name("Tracking"),
                35.0,
                FloatRange::Skewed {
                    min: 5.0,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_unit(" ms"),

            attack: FloatParam::new(
                name("Attack"),
                10.0,
                FloatRange::Skewed {
                    min: 0.1,
                    max: 300.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms"),

            release: FloatParam::new(
                name("Release"),
                100.0,
                FloatRange::Skewed {
                    min: 1.0,
                    max: 2000.0,
                    factor: FloatRange::skew_factor(-2.0),
                },
            )
            .with_unit(" ms"),

            clarity: FloatParam::new(
                name("Clarity"),
                0.85,
                FloatRange::Linear {
                    min: 0.5,
                    max: 0.99,
                },
            ),

            gate: FloatParam::new(
                name("Detector gate"),
                -55.0,
                FloatRange::Linear {
                    min: -80.0,
                    max: -20.0,
                },
            )
            .with_unit(" dBFS"),

            mix: FloatParam::new(
                name("Mix"),
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" %"),

            output: FloatParam::new(
                name("Output"),
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            exists: BoolParam::new(name("F0 exists"), true),
            enabled: BoolParam::new(name("F0 enabled"), true),
            ratio: ratio_param(name("F0 ratio")),
            kind: EnumParam::new(name("F0 type"), FilterKind::Peak),

            additional: std::array::from_fn(|i| BandParams::new(i + 2, prefix)),
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

fn target_frequency_param(name: &str, default: f32) -> FloatParam {
    FloatParam::new(
        name,
        default,
        FloatRange::Skewed {
            min: MIN_HZ,
            max: MAX_HZ,
            factor: FloatRange::skew_factor(-1.0),
        },
    )
    .with_unit(" Hz")
    .with_value_to_string(formatters::v2s_f32_rounded(2))
}

impl BandParams {
    fn new(number: usize, prefix: &str) -> Self {
        Self {
            gain: FloatParam::new(
                format!("{prefix}Band {number} gain"),
                0.0,
                FloatRange::Linear {
                    min: -24.0,
                    max: 24.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_rounded(1)),

            q: FloatParam::new(
                format!("{prefix}Band {number} Q"),
                3.0,
                FloatRange::Skewed {
                    min: 0.3,
                    max: 16.0,
                    factor: FloatRange::skew_factor(-1.0),
                },
            )
            .with_value_to_string(formatters::v2s_f32_rounded(2)),

            ratio: ratio_param(format!("{prefix}Band {number} ratio")),
            kind: EnumParam::new(format!("{prefix}Band {number} type"), FilterKind::Peak),

            exists: BoolParam::new(format!("{prefix}Band {number} exists"), false),
            enabled: BoolParam::new(format!("{prefix}Band {number} enabled"), true),
        }
    }
}

#[derive(Clone, Copy)]
pub struct BandRef<'a> {
    pub dynamics: &'a DynamicParams,
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
            dynamics: self.dynamics.control(),
            gain: self.gain.value(),
            q: self.q.value(),
            ratio: self.ratio.value(),
            kind: self.kind.value(),
            exists: self.exists.value(),
            enabled: self.enabled.value(),
        }
    }
}

impl ProcessingParams {
    pub fn target(&self) -> Target {
        Target {
            mode: self.target_mode.value(),
            minimum: self.target_min.value(),
            maximum: self.target_max.value(),
            reference: self.target_reference.value(),
            tolerance: self.target_tolerance.value(),
        }
    }

    pub fn band(&self, index: usize) -> BandRef<'_> {
        if index == 0 {
            BandRef {
                dynamics: &self.dynamics[index],
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
                dynamics: &self.dynamics[index],
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
