use crate::{
    analyzer::Analyzer,
    band::{BAND_COUNT, Band, BandControl},
    detector::{Detector, Estimate},
    filter::{Coefficients, Svf},
    lookahead::{Decision, Lookahead, LookaheadTime},
    midi::{MidiMode, MidiTracker, PitchSource},
    mode::OperatingMode,
    pitch::{DetectionMode, DetectionRange, Target},
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
    pub infinite_release: bool,
    pub clarity: f32,
    pub gate_db: f32,
    pub mix: f32,
    pub output_db: f32,
    pub bypass: bool,
    /// Zero-based point being auditioned, if any: the output is narrowed to a
    /// window around that point. The audio thread gets this from a shared
    /// atomic rather than a parameter, so the host's parameter list is untouched.
    pub audition: Option<usize>,
    /// Half width of that window in hertz.
    pub listen_half_width: f32,
}

impl Default for Controls {
    fn default() -> Self {
        let mut bands = std::array::from_fn(|i| BandControl {
            midi_channel: i as u8,
            ..BandControl::default()
        });
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
            infinite_release: false,
            clarity: 0.85,
            gate_db: -55.0,
            mix: 1.0,
            output_db: 0.0,
            bypass: false,
            audition: None,
            listen_half_width: LISTEN_HALF_WIDTH_DEFAULT,
        }
    }
}

/// Half the width of the audition window. The window is fixed in hertz rather
/// than relative to the point, so a low point is heard as wide as a high one.
pub const LISTEN_HALF_WIDTH_DEFAULT: f32 = 50.0;
/// The editor lets this be dragged from a few hertz to most of an octave.
pub const LISTEN_HALF_WIDTH_RANGE: [f32; 2] = [10.0, 500.0];
const LISTEN_MIN_HZ: f32 = 20.0;

pub struct Engine {
    pub mode: OperatingMode,
    pub pitch_source: PitchSource,
    pub midi_mode: MidiMode,
    midi: MidiTracker,
    point_log_frequencies: [f64; BAND_COUNT],
    point_frequencies: [f32; BAND_COUNT],
    point_voiced: [bool; BAND_COUNT],
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
    listen_filters: [Svf; 2],
}

impl Engine {
    pub fn new(sample_rate: f32) -> Self {
        let detector = Detector::new(sample_rate);
        let lookahead = Lookahead::with_capacity(
            sample_rate,
            detector.maximum_decision_age_samples(),
            detector.longest_decision_age_samples(),
        );

        Self {
            mode: OperatingMode::Track,
            visualize: false,
            pitch_source: PitchSource::default(),
            midi_mode: MidiMode::default(),
            midi: MidiTracker::default(),
            point_log_frequencies: [150.0_f64.ln(); BAND_COUNT],
            point_frequencies: [150.0; BAND_COUNT],
            point_voiced: [false; BAND_COUNT],
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
            listen_filters: [Svf::default(); 2],
        }
    }

    pub fn reset(&mut self) {
        self.midi.reset();
        self.reset_audio();
    }

    fn reset_audio(&mut self) {
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
        self.point_log_frequencies.fill(150.0_f64.ln());
        self.point_frequencies.fill(150.0);
        self.point_voiced.fill(false);
    }

    pub fn configure_lookahead(&mut self, enabled: bool, duration: LookaheadTime) {
        if self.lookahead.configure(enabled, duration) {
            self.reset_audio();
        }
    }

    pub fn configure_detection_range(&mut self, range: DetectionRange) {
        if self.detector.configure_range(range) {
            if self.pitch_source == PitchSource::Audio {
                self.lookahead
                    .set_minimum(self.detector.maximum_decision_age_samples());
                self.reset_audio();
            }
        }
    }

    pub fn configure_pitch_source(&mut self, source: PitchSource) {
        if self.pitch_source != source {
            self.pitch_source = source;
            self.lookahead.set_minimum(match source {
                PitchSource::Audio => self.detector.maximum_decision_age_samples(),
                PitchSource::Midi => 0,
            });
            self.reset_audio();
        }
    }

    pub fn set_midi_bend_range(&mut self, semitones: f32) {
        self.midi.set_bend_range(semitones);
    }

    pub fn configure_midi_mode(&mut self, mode: MidiMode) {
        if self.midi_mode != mode {
            self.midi_mode = mode;
            self.reset_audio();
        }
    }

    pub fn configure_midi_points(&mut self, mask: u8) {
        self.midi.set_point_mask(mask);
    }

    pub fn point_pitch(&self, index: usize) -> f32 {
        if self.pitch_source == PitchSource::Midi && self.midi_mode != MidiMode::Mono {
            self.point_frequencies[index]
        } else {
            self.frequency
        }
    }

    pub fn midi_active_mask(&self) -> u32 {
        self.point_voiced
            .iter()
            .enumerate()
            .fold(0, |mask, (i, voiced)| mask | (u32::from(*voiced) << i))
    }

    pub fn handle_midi(&mut self, event: nice_plug::prelude::NoteEvent<()>) {
        self.midi.handle(event);
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
        if self.pitch_source == PitchSource::Midi {
            self.configure_midi_points(
                controls
                    .bands
                    .iter()
                    .enumerate()
                    .fold(0, |mask, (i, b)| mask | (u8::from(b.exists) << i)),
            );
        }
        let changed_source = self.mode != controls.mode
            || (self.pitch_source == PitchSource::Audio
                && self.detector.mode != controls.detection);

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

        let decision = if self.pitch_source == PitchSource::Midi {
            let (frequency, midi_frequencies) = match self.midi_mode {
                MidiMode::Mono => (self.midi.frequency(), None),
                MidiMode::Poly => (self.midi.frequency(), Some(self.midi.point_frequencies())),
                MidiMode::ByChannel => {
                    let (frequency, points) = self
                        .midi
                        .channel_points(std::array::from_fn(|i| controls.bands[i].midi_channel));
                    (frequency, Some(points))
                }
            };
            Some(Decision {
                midi_frequencies,
                dated: true,
                age: 0,
                prelock_age: None,
                estimate: Estimate {
                    frequency,
                    clarity: f32::from(frequency > 0.0),
                    rms: 0.0,
                },
            })
        } else if self.detector.push(detection_input) {
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
                        midi_frequencies: None,
                        dated: true,
                        age: center,
                        prelock_age: acquiring.then_some(start),
                        estimate,
                    }
                } else {
                    Decision {
                        midi_frequencies: None,
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

        let Some(frame) = self.lookahead.push(
            input,
            detection_input,
            controls,
            decision,
            changed_source && self.pitch_source == PitchSource::Audio,
        ) else {
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
        self.source_started = self.estimate.frequency > 0.0
            && (self.source_started
                || frame.source_active
                || self.pitch_source == PitchSource::Midi);

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

        if let Some(frequencies) = frame.midi_frequencies {
            for (i, frequency) in frequencies.into_iter().enumerate() {
                self.point_voiced[i] = frequency > 0.0;
                if self.point_voiced[i] {
                    let target = f64::from(frequency).ln();
                    if self.bands[i].wet == 0.0 {
                        self.point_log_frequencies[i] = target;
                    }
                    self.point_log_frequencies[i] +=
                        self.tracking_alpha * (target - self.point_log_frequencies[i]);
                    self.point_frequencies[i] = self.point_log_frequencies[i].exp() as f32;
                }
            }
        }

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

        let per_point = self.pitch_source == PitchSource::Midi && self.midi_mode != MidiMode::Mono;
        // Audition narrows the finished output to the selected point's
        // neighbourhood instead of muting the other points, so the range is
        // heard exactly as the chain leaves it.
        let audition = controls
            .audition
            .filter(|&index| index < BAND_COUNT && controls.bands[index].exists);
        for (i, (band, control)) in self.bands.iter_mut().zip(controls.bands).enumerate() {
            let mut envelope_steps = self.envelope_steps;
            if controls.infinite_release && control.exists && control.enabled {
                // Hold the existing amount on pitch loss; explicit band removal
                // and bypass still use their normal release paths.
                envelope_steps[1] = 0.0;
            }
            wet = band.process(
                wet,
                frame.source,
                control,
                if per_point {
                    self.point_frequencies[i]
                } else {
                    self.frequency
                },
                if per_point {
                    self.point_voiced[i]
                } else {
                    self.source_started
                },
                self.sample_rate,
                self.control_alpha,
                envelope_steps,
            );
        }

        self.gain = self.bands[0].gain;
        self.q = self.bands[0].q;

        let output_gain = 10.0_f32.powf(self.output_db / 20.0);
        // The audition window is centred on the point's own filter frequency, so
        // the range is judged in place rather than by muting the rest of the
        // chain. Its width is set from the strip and can be dragged.
        let half_width = controls
            .listen_half_width
            .clamp(LISTEN_HALF_WIDTH_RANGE[0], LISTEN_HALF_WIDTH_RANGE[1]);
        let listen = audition.map(|index| {
            let center = self.bands[index]
                .frequency
                .clamp(LISTEN_MIN_HZ, self.sample_rate * 0.45);
            Coefficients::bandpass(self.sample_rate, center, center / (2.0 * half_width))
        });
        let output = std::array::from_fn(|ch| {
            let mixed = (input[ch] + self.mix * (wet[ch] - input[ch])) * output_gain;
            let mixed = match listen {
                Some(coefficients) => self.listen_filters[ch].process(mixed, coefficients),
                None => mixed,
            };
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
