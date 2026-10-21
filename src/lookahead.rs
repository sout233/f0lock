use crate::{detector::Estimate, engine::Controls};
use nice_plug::prelude::Enum;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LookaheadTime {
    #[id = "minimum"]
    #[name = "Minimum"]
    Minimum,
    #[default]
    #[id = "ms150"]
    #[name = "150 ms"]
    Ms150,
    #[id = "ms200"]
    #[name = "200 ms"]
    Ms200,
    #[id = "ms300"]
    #[name = "300 ms"]
    Ms300,
}

impl LookaheadTime {
    pub const ALL: [Self; 4] = [Self::Minimum, Self::Ms150, Self::Ms200, Self::Ms300];

    pub fn label(self) -> &'static str {
        match self {
            Self::Minimum => "Minimum",
            Self::Ms150 => "150 ms",
            Self::Ms200 => "200 ms",
            Self::Ms300 => "300 ms",
        }
    }

    pub fn samples(self, sample_rate: f32, minimum: usize) -> usize {
        let ms = match self {
            Self::Minimum => return minimum,
            Self::Ms150 => 150.0,
            Self::Ms200 => 200.0,
            Self::Ms300 => 300.0,
        };
        ((sample_rate * ms * 0.001).round() as usize).max(minimum)
    }
}

#[derive(Clone, Copy, Default)]
pub struct Frame {
    pub audio: [f32; 2],
    pub source: [f32; 2],
    pub controls: Controls,
    pub estimate: Option<Estimate>,
    pub source_active: bool,
    pub prelock: Option<Estimate>,
}

pub struct Decision {
    pub dated: bool,
    pub age: usize,
    pub prelock_age: Option<usize>,
    pub estimate: Estimate,
}

/// Audio, automation and dated pitch decisions share the same sample clock.
pub struct Lookahead {
    frames: Vec<Frame>,
    time: u64,
    enabled: bool,
    sample_rate: f32,
    minimum: usize,
    delay: usize,
    gate_db: f32,
    gate_amplitude: f32,
}

impl Lookahead {
    pub fn new(sample_rate: f32, maximum_decision_age: usize) -> Self {
        let capacity = LookaheadTime::Ms300.samples(sample_rate, maximum_decision_age) + 1;

        Self {
            frames: vec![Frame::default(); capacity],
            time: 0,
            enabled: false,
            sample_rate,
            minimum: maximum_decision_age,
            delay: LookaheadTime::default().samples(sample_rate, maximum_decision_age),
            gate_db: -55.0,
            gate_amplitude: 10.0_f32.powf(-55.0 / 20.0),
        }
    }

    pub fn latency_samples(&self) -> usize {
        if self.enabled { self.delay } else { 0 }
    }

    pub fn configure(&mut self, enabled: bool, duration: LookaheadTime) -> bool {
        let previous = self.latency_samples();
        self.enabled = enabled;
        self.delay = duration.samples(self.sample_rate, self.minimum);
        previous != self.latency_samples()
    }

    pub fn reset(&mut self) {
        self.frames.fill(Frame::default());
        self.time = 0;
    }

    pub fn push(
        &mut self,
        audio: [f32; 2],
        source: [f32; 2],
        controls: Controls,
        decision: Option<Decision>,
        changed_source: bool,
    ) -> Option<Frame> {
        if !self.enabled {
            return Some(Frame {
                audio,
                source,
                controls,
                estimate: if changed_source {
                    Some(Estimate::default())
                } else {
                    decision.map(|d| d.estimate)
                },
                source_active: true,
                prelock: None,
            });
        }

        if controls.gate_db != self.gate_db {
            self.gate_db = controls.gate_db;
            self.gate_amplitude = 10.0_f32.powf(self.gate_db / 20.0);
        }

        let capacity = self.frames.len() as u64;
        let index = (self.time % capacity) as usize;
        self.frames[index] = Frame {
            audio,
            source,
            controls,
            estimate: changed_source.then_some(Estimate::default()),
            source_active: source[0].abs().max(source[1].abs()) >= self.gate_amplitude,
            prelock: None,
        };

        if let Some(Decision {
            dated: true,
            age,
            prelock_age,
            estimate,
        }) = decision
        {
            assert!(
                age <= self.latency_samples(),
                "Pitch decision predates the audio buffer"
            );
            assert!(
                age as u64 <= self.time,
                "Pitch decision predates stream start"
            );
            let index = ((self.time - age as u64) % capacity) as usize;
            self.frames[index].estimate = Some(estimate);

            if let Some(start) = prelock_age {
                assert!(start >= age && start <= self.latency_samples());
                assert!(start as u64 <= self.time);

                // Only the leading half of the same observed window can use
                // its first voiced pitch to prepare an otherwise dry filter.
                for offset in age + 1..=start {
                    let index = ((self.time - offset as u64) % capacity) as usize;
                    self.frames[index].prelock = Some(estimate);
                }
            }
        }

        let output = if self.time >= self.latency_samples() as u64 {
            Some(self.frames[((self.time - self.latency_samples() as u64) % capacity) as usize])
        } else {
            None
        };
        self.time += 1;
        output
    }

    pub fn analysis_controls(&self, age: usize, live: Controls) -> Controls {
        if !self.enabled {
            return live;
        }

        assert!(age <= self.latency_samples() && age as u64 <= self.time);
        self.frames[((self.time - age as u64) % self.frames.len() as u64) as usize].controls
    }
}
