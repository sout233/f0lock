use crate::{detector::Estimate, engine::Controls};

#[derive(Clone, Copy, Default)]
pub struct Frame {
    pub audio: [f32; 2],
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
    gate_db: f32,
    gate_amplitude: f32,
}

impl Lookahead {
    pub fn new(sample_rate: f32, maximum_decision_age: usize) -> Self {
        let delay = ((sample_rate * 0.150).round() as usize).max(maximum_decision_age);

        Self {
            frames: vec![Frame::default(); delay + 1],
            time: 0,
            enabled: false,
            gate_db: -55.0,
            gate_amplitude: 10.0_f32.powf(-55.0 / 20.0),
        }
    }

    pub fn latency_samples(&self) -> usize {
        if self.enabled {
            self.frames.len() - 1
        } else {
            0
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) -> bool {
        let changed = self.enabled != enabled;
        self.enabled = enabled;
        changed
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
