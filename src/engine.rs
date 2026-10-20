use crate::{
    analyzer::Analyzer,
    band::{BAND_COUNT, Band, BandControl},
    detector::{Detector, Estimate},
};

#[derive(Clone, Copy)]
pub struct Controls {
    pub bands: [BandControl; BAND_COUNT],
    pub tracking_ms: f32,
    pub clarity: f32,
    pub gate_db: f32,
    pub mix: f32,
    pub output_db: f32,
    pub bypass: bool,
}

impl Default for Controls {
    fn default() -> Self {
        let mut bands = [BandControl::default(); BAND_COUNT];
        bands[0].exists = true;
        bands[0].gain = -6.0;

        Self {
            bands,
            tracking_ms: 35.0,
            clarity: 0.85,
            gate_db: -55.0,
            mix: 1.0,
            output_db: 0.0,
            bypass: false,
        }
    }
}

pub struct Engine {
    pub visualize: bool,
    pub detector: Detector,
    pub analyzer: Analyzer,
    pub estimate: Estimate,
    pub sample_rate: f32,
    pub frequency: f32,
    pub gain: f32,
    pub q: f32,
    pub input_peak: f32,
    pub output_peak: f32,
    pub bands: [Band; BAND_COUNT],
    log_frequency: f64,
    target_log_frequency: f64,
    tracking_ms: f32,
    tracking_alpha: f64,
    mix: f32,
    output_db: f32,
    bypass: f32,
    control_alpha: f32,
    peak_decay: f32,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            visualize: false,
            detector: Detector::new(sample_rate),
            analyzer: Analyzer::new(sample_rate),
            estimate: Estimate::default(),
            sample_rate,
            frequency: 150.0,
            gain: 0.0,
            q: 3.0,
            input_peak: 0.0,
            output_peak: 0.0,
            bands: std::array::from_fn(|_| Band::new(sample_rate)),
            log_frequency: 150.0_f64.ln(),
            target_log_frequency: 150.0_f64.ln(),
            tracking_ms: 35.0,
            tracking_alpha: 1.0 - (-1.0 / (0.035 * f64::from(sample_rate))).exp(),
            mix: 1.0,
            output_db: 0.0,
            bypass: 0.0,
            control_alpha: 1.0 - (-1.0 / (0.01 * sample_rate)).exp(),
            peak_decay: (-1.0 / (0.3 * sample_rate)).exp(),
        }
    }

    pub fn reset(&mut self) {
        self.detector.reset();
        self.analyzer.reset();
        self.estimate = Estimate::default();
        self.bands = std::array::from_fn(|_| Band::new(self.sample_rate));
        self.frequency = 150.0;
        self.log_frequency = 150.0_f64.ln();
        self.target_log_frequency = self.log_frequency;
        self.gain = 0.0;
        self.q = 3.0;
        self.mix = 1.0;
        self.output_db = 0.0;
        self.bypass = 0.0;

        self.input_peak = 0.0;
        self.output_peak = 0.0;
    }

    pub fn process(&mut self, input: [f32; 2], controls: Controls) -> [f32; 2] {
        self.analyzer.push(input, self.visualize);

        if self.detector.push(input) {
            self.estimate = self.detector.analyze(controls.gate_db, controls.clarity);

            if self.estimate.frequency > 0.0 {
                self.target_log_frequency = f64::from(self.estimate.frequency).ln();
            }
        }

        if controls.tracking_ms != self.tracking_ms {
            self.tracking_ms = controls.tracking_ms;
            self.tracking_alpha = 1.0
                - (-1.0 / (f64::from(self.tracking_ms) * 0.001 * f64::from(self.sample_rate)))
                    .exp();
        }

        self.log_frequency +=
            self.tracking_alpha * (self.target_log_frequency - self.log_frequency);
        self.frequency = self.log_frequency.exp() as f32;
        self.mix += self.control_alpha * (controls.mix - self.mix);
        self.output_db += self.control_alpha * (controls.output_db - self.output_db);

        let bypass_step = 1.0 / (0.01 * self.sample_rate);
        self.bypass += (f32::from(controls.bypass) - self.bypass).clamp(-bypass_step, bypass_step);

        let mut wet = input;

        for (band, control) in self.bands.iter_mut().zip(controls.bands) {
            wet = band.process(
                wet,
                control,
                self.frequency,
                self.estimate.frequency > 0.0,
                self.sample_rate,
                self.control_alpha,
            );
        }

        self.gain = self.bands[0].gain;
        self.q = self.bands[0].q;

        let output_gain = 10.0_f32.powf(self.output_db / 20.0);
        let output = std::array::from_fn(|ch| {
            let mixed = (input[ch] + self.mix * (wet[ch] - input[ch])) * output_gain;
            input[ch] + (1.0 - self.bypass) * (mixed - input[ch])
        });

        self.input_peak = input[0]
            .abs()
            .max(input[1].abs())
            .max(self.input_peak * self.peak_decay);
        self.output_peak = output[0]
            .abs()
            .max(output[1].abs())
            .max(self.output_peak * self.peak_decay);
        output
    }
}
