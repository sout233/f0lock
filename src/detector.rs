use crate::filter::{Coefficients, Svf};
use crate::pitch::{DetectionMode, DetectionRange, LOOKAHEAD, PitchTracking, Target, TargetMode};
use pitch_estimate::{McLeodDetector, PitchDetector};

pub const WINDOW: usize = 1024;
// Half the window covers a 25 Hz period even at decimated rates near 24 kHz.
pub const EXTENDED_WINDOW: usize = 2048;
pub const HOP: usize = 128;

#[derive(Clone, Copy, Default, Debug)]
pub struct Estimate {
    pub frequency: f32,
    pub clarity: f32,
    pub rms: f32,
}

struct DetectorCore<
    const WINDOW: usize,
    const BINS: usize,
    const PERIODIC_BINS: usize,
    const STATES: usize,
> {
    minimum: f32,
    pub mode: DetectionMode,
    pub target: Target,
    tracking: PitchTracking<BINS, PERIODIC_BINS, STATES>,
    pub rate: f32,
    decimation: usize,
    phase: usize,
    filters: [[Svf; 2]; 2],
    lowpass: [Coefficients; 2],
    configured_mode: DetectionMode,
    wide_filters: [[Svf; 4]; 2],
    wide_lowpass: [Coefficients; 4],
    rings: [[f32; WINDOW]; 2],
    position: usize,
    filled: usize,
    hop: usize,
    samples_seen: u64,
    decision_ready: bool,
    decision_lag: usize,
    mpm: McLeodDetector<f32>,
    frame: [f32; WINDOW],
}

impl<const WINDOW: usize, const BINS: usize, const PERIODIC_BINS: usize, const STATES: usize>
    DetectorCore<WINDOW, BINS, PERIODIC_BINS, STATES>
{
    fn new(sample_rate: f32, minimum: f32) -> Self {
        let decimation = (sample_rate / 12000.0).floor().max(1.0) as usize;
        let rate = sample_rate / decimation as f32;

        let mpm = McLeodDetector::new(WINDOW, (rate / minimum * 1.6).ceil() as usize)
            .expect("Invalid MPM window configuration")
            .with_k(0.93)
            .with_clarity_threshold(0.0);

        Self {
            minimum,
            mode: DetectionMode::default(),
            target: Target::default(),
            tracking: PitchTracking::new(rate, WINDOW, HOP, minimum),
            rate,
            decimation,
            phase: 0,
            filters: [[Svf::default(); 2]; 2],
            lowpass: [
                Coefficients::lowpass(sample_rate, 1000.0, 0.5411961),
                Coefficients::lowpass(sample_rate, 1000.0, 1.306563),
            ],
            configured_mode: DetectionMode::default(),
            wide_filters: [[Svf::default(); 4]; 2],
            wide_lowpass: [0.5097956, 0.6013449, 0.8999762, 2.5629154]
                .map(|q| Coefficients::lowpass(sample_rate, (rate * 0.3).min(4000.0), q)),
            rings: [[0.0; WINDOW]; 2],
            position: 0,
            filled: 0,
            hop: 0,
            samples_seen: 0,
            decision_ready: false,
            decision_lag: 0,
            mpm,
            frame: [0.0; WINDOW],
        }
    }

    pub fn reset(&mut self) {
        self.tracking.reset();
        self.filters = [[Svf::default(); 2]; 2];
        self.wide_filters = [[Svf::default(); 4]; 2];
        self.rings = [[0.0; WINDOW]; 2];
        self.position = 0;
        self.filled = 0;

        self.phase = 0;

        self.hop = 0;
        self.samples_seen = 0;
        self.decision_ready = false;
    }

    pub fn maximum_decision_age_samples(&self) -> usize {
        (WINDOW + LOOKAHEAD * HOP) * self.decimation - 1
    }

    /// Start and center ages of the returned analysis window. Delayed
    /// decoders describe a window three hops behind the newest one.
    pub fn decision_ages(&self) -> Option<(usize, usize)> {
        let age = (WINDOW + self.decision_lag * HOP) * self.decimation - 1;
        (self.decision_ready && self.samples_seen > age as u64)
            .then_some((age, age - WINDOW * self.decimation / 2))
    }

    pub fn analysis_center_age_samples(&self) -> usize {
        WINDOW * self.decimation / 2 - 1
    }

    pub fn push(&mut self, samples: [f32; 2]) -> bool {
        if self.mode != self.configured_mode {
            self.reset();
            self.configured_mode = self.mode;
        }

        self.samples_seen += 1;

        // Periodicity modes retain their 1 kHz path; harmonic salience uses
        // wider anti-alias filtering. Independent channels avoid cancellation.
        let mut filtered = [0.0; 2];

        for ch in 0..2 {
            if self.mode == DetectionMode::SpectralMelody {
                let mut value = samples[ch];

                for (filter, coefficients) in
                    self.wide_filters[ch].iter_mut().zip(self.wide_lowpass)
                {
                    value = filter.lowpass(value, coefficients);
                }

                filtered[ch] = value;
            } else {
                let first = self.filters[ch][0].lowpass(samples[ch], self.lowpass[0]);
                filtered[ch] = self.filters[ch][1].lowpass(first, self.lowpass[1]);
            }
        }

        self.phase += 1;

        if self.phase != self.decimation {
            return false;
        }

        self.phase = 0;

        for (ch, value) in filtered.into_iter().enumerate() {
            self.rings[ch][self.position] = value;
        }

        self.position = (self.position + 1) % WINDOW;
        self.filled = (self.filled + 1).min(WINDOW);
        self.hop += 1;

        if self.hop < HOP {
            return false;
        }

        self.hop = 0;
        self.filled == WINDOW
    }

    pub fn analyze(&mut self, gate_db: f32, threshold: f32) -> Estimate {
        self.decision_ready = true;
        self.decision_lag = 0;
        self.tracking.set_mode(self.mode);
        let means: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().sum::<f32>() / WINDOW as f32);
        let powers: [f32; 2] =
            std::array::from_fn(|ch| self.rings[ch].iter().map(|v| (v - means[ch]).powi(2)).sum());
        let channel = usize::from(powers[1] > powers[0]);
        let mean = means[channel];

        for i in 0..WINDOW {
            self.frame[i] = self.rings[channel][(self.position + i) % WINDOW] - mean;
        }

        let rms = (self.frame.iter().map(|v| v * v).sum::<f32>() / WINDOW as f32).sqrt();

        if rms < 10.0_f32.powf(gate_db / 20.0) {
            self.tracking.reset();
            return Estimate {
                rms,
                ..Estimate::default()
            };
        }

        if self.mode != DetectionMode::Mpm || self.target.mode != TargetMode::Auto {
            let (frequency, clarity) = self.tracking.analyze(
                &self.frame,
                self.rate,
                &mut self.mpm,
                threshold,
                self.target,
            );
            self.decision_ready = self.tracking.decision_ready;
            self.decision_lag = if matches!(
                self.mode,
                DetectionMode::Pyin | DetectionMode::SpectralMelody
            ) {
                LOOKAHEAD
            } else {
                0
            };

            return Estimate {
                frequency,
                clarity,
                rms,
            };
        }

        match self.mpm.detect(&self.frame, self.rate.round() as u32) {
            Some(pitch) => {
                let hz = pitch.frequency * self.rate / self.rate.round();
                let valid = pitch.clarity >= threshold && (self.minimum..=1000.0).contains(&hz);
                Estimate {
                    frequency: if valid { hz } else { 0.0 },
                    clarity: pitch.clarity,
                    rms,
                }
            }
            None => Estimate {
                rms,
                ..Estimate::default()
            },
        }
    }
}

/// Both profiles are prepared off the audio thread; only the selected one runs.
pub struct Detector {
    pub mode: DetectionMode,
    pub target: Target,
    pub rate: f32,
    range: DetectionRange,
    standard: Box<DetectorCore<WINDOW, 640, 520, 1280>>,
    extended: Box<DetectorCore<EXTENDED_WINDOW, 760, 640, 1520>>,
}

macro_rules! dispatch_detector {
    ($self:ident, $method:ident($($arg:expr),* $(,)?)) => {
        match $self.range {
            DetectionRange::Standard => $self.standard.$method($($arg),*),
            DetectionRange::Low => $self.extended.$method($($arg),*),
        }
    };
}

impl Detector {
    pub fn new(sample_rate: f32) -> Self {
        let standard = Box::new(DetectorCore::new(
            sample_rate,
            DetectionRange::Standard.minimum(),
        ));
        let rate = standard.rate;
        Self {
            mode: DetectionMode::default(),
            target: Target::default(),
            rate,
            range: DetectionRange::default(),
            standard,
            extended: Box::new(DetectorCore::new(
                sample_rate,
                DetectionRange::Low.minimum(),
            )),
        }
    }

    pub fn configure_range(&mut self, range: DetectionRange) -> bool {
        if self.range == range {
            return false;
        }
        self.range = range;
        self.reset();
        true
    }

    pub fn window_samples(&self) -> usize {
        match self.range {
            DetectionRange::Standard => WINDOW,
            DetectionRange::Low => EXTENDED_WINDOW,
        }
    }

    pub fn extended_decision_age_samples(&self) -> usize {
        self.extended.maximum_decision_age_samples()
    }

    pub fn reset(&mut self) {
        dispatch_detector!(self, reset());
    }

    pub fn maximum_decision_age_samples(&self) -> usize {
        dispatch_detector!(self, maximum_decision_age_samples())
    }

    pub fn decision_ages(&self) -> Option<(usize, usize)> {
        dispatch_detector!(self, decision_ages())
    }

    pub fn analysis_center_age_samples(&self) -> usize {
        dispatch_detector!(self, analysis_center_age_samples())
    }

    pub fn push(&mut self, samples: [f32; 2]) -> bool {
        match self.range {
            DetectionRange::Standard => self.standard.mode = self.mode,
            DetectionRange::Low => self.extended.mode = self.mode,
        }
        dispatch_detector!(self, push(samples))
    }

    pub fn analyze(&mut self, gate_db: f32, threshold: f32) -> Estimate {
        let target = self.target.for_minimum(self.range.minimum());
        match self.range {
            DetectionRange::Standard => self.standard.target = target,
            DetectionRange::Low => self.extended.target = target,
        }
        dispatch_detector!(self, analyze(gate_db, threshold))
    }
}
