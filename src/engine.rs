use crate::{
    analyzer::Analyzer,
    band::{BAND_COUNT, Band, BandControl},
    detector::{Detector, Estimate},
    lookahead::{Decision, Lookahead},
    mode::OperatingMode,
    pitch::{DetectionMode, Target},
};

#[derive(Clone, Copy)]
pub struct Controls {
    pub mode: OperatingMode,
    pub detection: DetectionMode,
    pub target: Target,
    pub bands: [BandControl; BAND_COUNT],
    pub tracking_ms: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
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
            mode: OperatingMode::Track,
            detection: DetectionMode::default(),
            target: Target::default(),
            bands,
            tracking_ms: 35.0,
            attack_ms: 10.0,
            release_ms: 100.0,
            clarity: 0.85,
            gate_db: -55.0,
            mix: 1.0,
            output_db: 0.0,
            bypass: false,
        }
    }
}

pub struct Engine {
    pub mode: OperatingMode,
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
    envelope_ms: [f32; 2],
    envelope_steps: [f32; 2],
    mix: f32,
    output_db: f32,
    bypass: f32,
    control_alpha: f32,
    peak_decay: f32,
    lookahead: Lookahead,
    source_started: bool,
    detector_voiced: bool,
    prelocking: bool,
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        let detector = Detector::new(sample_rate);
        let lookahead = Lookahead::new(sample_rate, detector.maximum_decision_age_samples());

        Self {
            mode: OperatingMode::Track,
            visualize: false,
            detector,
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
            envelope_ms: [10.0, 100.0],
            envelope_steps: [1.0 / (0.01 * sample_rate), 1.0 / (0.1 * sample_rate)],
            mix: 1.0,
            output_db: 0.0,
            bypass: 0.0,
            control_alpha: 1.0 - (-1.0 / (0.01 * sample_rate)).exp(),
            peak_decay: (-1.0 / (0.3 * sample_rate)).exp(),
            lookahead,
            source_started: false,
            detector_voiced: false,
            prelocking: false,
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
        self.lookahead.reset();
        self.source_started = false;
        self.detector_voiced = false;
        self.prelocking = false;
    }

    pub fn set_lookahead(&mut self, enabled: bool) {
        if self.lookahead.set_enabled(enabled) {
            self.reset();
        }
    }

    pub fn latency_samples(&self) -> usize {
        self.lookahead.latency_samples()
    }

    #[cfg(test)]
    pub fn process(&mut self, input: [f32; 2], controls: Controls) -> [f32; 2] {
        self.process_with_sidechain(input, None, controls)
    }

    pub fn process_with_sidechain(
        &mut self,
        input: [f32; 2],
        sidechain: Option<[f32; 2]>,
        controls: Controls,
    ) -> [f32; 2] {
        let changed_source = self.mode != controls.mode || self.detector.mode != controls.detection;

        if changed_source {
            self.detector_voiced = false;
        }

        if self.mode != controls.mode {
            self.mode = controls.mode;
            self.detector.reset();
        }

        let detection_input = match controls.mode {
            OperatingMode::Track => input,
            // An unrouted external bus is silence, never a main-input detector.
            OperatingMode::Sidechain => sidechain.unwrap_or([0.0; 2]),
        };

        self.detector.mode = controls.detection;

        let decision = if self.detector.push(detection_input) {
            let analysis = self
                .lookahead
                .analysis_controls(self.detector.analysis_center_age_samples(), controls);
            self.detector.target = analysis.target;
            let estimate = self.detector.analyze(analysis.gate_db, analysis.clarity);
            let acquiring = estimate.frequency > 0.0 && !self.detector_voiced;
            self.detector_voiced = estimate.frequency > 0.0;
            Some(
                if let Some((start, center)) = self.detector.decision_ages() {
                    Decision {
                        dated: true,
                        age: center,
                        prelock_age: acquiring.then_some(start),
                        estimate,
                    }
                } else {
                    Decision {
                        dated: false,
                        age: 0,
                        prelock_age: None,
                        estimate,
                    }
                },
            )
        } else {
            None
        };

        let Some(frame) =
            self.lookahead
                .push(input, detection_input, controls, decision, changed_source)
        else {
            return [0.0; 2];
        };
        let input = frame.audio;
        let controls = frame.controls;
        self.analyzer.push(input, self.visualize);

        if frame.prelock.is_none() {
            self.prelocking = false;
        } else if self.estimate.frequency == 0.0
            && self.bands.iter().all(|band| band.wet == 0.0)
            && frame.source_active
        {
            self.prelocking = true;
        }

        if let Some(estimate) = if self.prelocking {
            frame.prelock
        } else {
            frame.estimate
        } {
            self.estimate = estimate;
        }

        if self.estimate.frequency > 0.0 {
            self.target_log_frequency = f64::from(self.estimate.frequency).ln();

            // No audible filter state exists while every band is fully dry.
            // Initialize before Attack instead of sweeping from a placeholder.
            if self.bands.iter().all(|band| band.wet == 0.0) {
                self.log_frequency = self.target_log_frequency;
            }
        }

        // Lookahead can know a pitch before the buffered source starts. Wait
        // for its Gate crossing so Attack is not spent on preceding silence.
        self.source_started =
            self.estimate.frequency > 0.0 && (self.source_started || frame.source_active);

        if controls.tracking_ms != self.tracking_ms {
            self.tracking_ms = controls.tracking_ms;
            self.tracking_alpha = 1.0
                - (-1.0 / (f64::from(self.tracking_ms) * 0.001 * f64::from(self.sample_rate)))
                    .exp();
        }

        if self.estimate.frequency > 0.0 {
            self.log_frequency +=
                self.tracking_alpha * (self.target_log_frequency - self.log_frequency);
        }
        self.frequency = self.log_frequency.exp() as f32;

        for (i, time) in [controls.attack_ms, controls.release_ms]
            .into_iter()
            .enumerate()
        {
            if time != self.envelope_ms[i] {
                self.envelope_ms[i] = time;
                self.envelope_steps[i] = 1.0 / (time * 0.001 * self.sample_rate);
            }
        }
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
                self.source_started,
                self.sample_rate,
                self.control_alpha,
                self.envelope_steps,
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
