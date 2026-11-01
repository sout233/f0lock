use nice_plug::midi::{Channel, Key, VoiceID};
use nice_plug::prelude::*;

#[derive(Enum, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum PitchSource {
    #[default]
    #[id = "audio"]
    #[name = "Audio"]
    Audio,
    #[id = "midi"]
    #[name = "MIDI"]
    Midi,
}

impl PitchSource {
    pub const ALL: [Self; 2] = [Self::Audio, Self::Midi];

    pub fn label(self) -> &'static str {
        match self {
            Self::Audio => "Audio",
            Self::Midi => "MIDI",
        }
    }
}

#[derive(Clone, Copy)]
struct HeldNote {
    channel: u8,
    key: u8,
    voice_id: VoiceID,
    tuning: f32,
    pressed: bool,
}

impl HeldNote {
    const EMPTY: Self = Self {
        channel: 0,
        key: 0,
        voice_id: VoiceID::Wildcard,
        tuning: 0.0,
        pressed: false,
    };

    fn matches(self, voice_id: VoiceID, channel: Channel, key: Key) -> bool {
        (voice_id.is_wildcard() || self.voice_id == voice_id)
            && (channel.is_wildcard() || channel.number() == Some(self.channel))
            && (key.is_wildcard() || key.number() == Some(self.key))
    }
}

/// Bounded note history and per-channel controllers; event handling never allocates.
pub struct MidiTracker {
    notes: [HeldNote; 2048],
    len: usize,
    bend: [f32; 16],
    sustain: [bool; 16],
    bend_range: f32,
    frequency: f32,
}

impl Default for MidiTracker {
    fn default() -> Self {
        Self {
            notes: [HeldNote::EMPTY; 2048],
            len: 0,
            bend: [0.0; 16],
            sustain: [false; 16],
            bend_range: 2.0,
            frequency: 0.0,
        }
    }
}

impl MidiTracker {
    pub fn reset(&mut self) {
        self.len = 0;
        self.bend.fill(0.0);
        self.sustain.fill(false);
        self.frequency = 0.0;
    }

    pub fn frequency(&self) -> f32 {
        self.frequency
    }

    pub fn set_bend_range(&mut self, semitones: f32) {
        if semitones.is_finite() && self.bend_range != semitones {
            self.bend_range = semitones.clamp(1.0, 24.0);
            self.refresh();
        }
    }

    fn retain(&mut self, mut keep: impl FnMut(HeldNote) -> bool) {
        let mut len = 0;
        for i in 0..self.len {
            let note = self.notes[i];
            if keep(note) {
                self.notes[len] = note;
                len += 1;
            }
        }
        self.len = len;
    }

    fn release(&mut self, voice_id: VoiceID, channel: Channel, key: Key) {
        for note in &mut self.notes[..self.len] {
            if note.matches(voice_id, channel, key) {
                note.pressed = false;
            }
        }
        let sustain = self.sustain;
        self.retain(|note| note.pressed || sustain[usize::from(note.channel)]);
    }

    fn refresh(&mut self) {
        self.frequency = if self.len == 0 {
            0.0
        } else {
            let note = self.notes[self.len - 1];
            let pitch = f32::from(note.key)
                + self.bend[usize::from(note.channel)] * self.bend_range
                + note.tuning;
            440.0 * 2.0_f32.powf((pitch - 69.0) / 12.0)
        };
    }

    pub fn handle(&mut self, event: NoteEvent<()>) {
        match event {
            NoteEvent::NoteOn {
                voice_id,
                channel,
                key,
                velocity,
                ..
            } => {
                let (Some(channel), Some(key)) = (channel.number(), key.number()) else {
                    return;
                };
                if channel >= 16 || key >= 128 || !velocity.is_finite() {
                    return;
                }
                if velocity <= 0.0 {
                    self.release(voice_id, Channel::Number(channel), Key::Number(key));
                } else {
                    self.retain(|note| {
                        !note.matches(voice_id, Channel::Number(channel), Key::Number(key))
                    });
                    if self.len == self.notes.len() {
                        self.notes.copy_within(1.., 0);
                        self.len -= 1;
                    }
                    self.notes[self.len] = HeldNote {
                        channel,
                        key,
                        voice_id,
                        tuning: 0.0,
                        pressed: true,
                    };
                    self.len += 1;
                }
            }
            NoteEvent::NoteOff {
                voice_id,
                channel,
                key,
                ..
            } => self.release(voice_id, channel, key),
            NoteEvent::Choke {
                voice_id,
                channel,
                key,
                ..
            } => {
                self.retain(|note| !note.matches(voice_id, channel, key));
            }
            NoteEvent::PolyTuning {
                voice_id,
                channel,
                key,
                tuning,
                ..
            } if tuning.is_finite() => {
                for note in &mut self.notes[..self.len] {
                    if note.matches(voice_id, channel, key) {
                        note.tuning = tuning.clamp(-128.0, 128.0);
                    }
                }
            }
            NoteEvent::MidiPitchBend { channel, value, .. }
                if channel < 16 && value.is_finite() =>
            {
                self.bend[usize::from(channel)] = 2.0 * value.clamp(0.0, 1.0) - 1.0;
            }
            NoteEvent::MidiCC {
                channel, cc, value, ..
            } if channel < 16 && value.is_finite() => {
                let index = usize::from(channel);
                match cc {
                    64 => {
                        self.sustain[index] = value >= 0.5;
                        if !self.sustain[index] {
                            self.retain(|note| note.channel != channel || note.pressed);
                        }
                    }
                    120 => self.retain(|note| note.channel != channel),
                    121 => {
                        self.bend[index] = 0.0;
                        self.sustain[index] = false;
                        self.retain(|note| note.channel != channel || note.pressed);
                    }
                    123 => self.release(VoiceID::Wildcard, Channel::Number(channel), Key::Wildcard),
                    _ => return,
                }
            }
            _ => return,
        }
        self.refresh();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on(channel: u8, key: u8) -> NoteEvent<()> {
        NoteEvent::NoteOn {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(channel),
            key: Key::Number(key),
            velocity: 1.0,
        }
    }

    fn off(channel: u8, key: u8) -> NoteEvent<()> {
        NoteEvent::NoteOff {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Number(channel),
            key: Key::Number(key),
            velocity: 0.0,
        }
    }

    #[test]
    fn last_note_priority_returns_to_held_notes_and_tracks_bend_per_channel() {
        let mut midi = MidiTracker::default();
        nice_assert_no_alloc::assert_no_alloc(|| {
            midi.handle(on(0, 57));
            midi.handle(on(1, 69));
            assert_eq!(midi.frequency(), 440.0);
            midi.handle(NoteEvent::MidiPitchBend {
                timing: 0,
                channel: 0,
                value: 1.0,
            });
            assert_eq!(midi.frequency(), 440.0);
            midi.handle(NoteEvent::MidiPitchBend {
                timing: 0,
                channel: 1,
                value: 0.0,
            });
            assert!((midi.frequency() - 391.9954).abs() < 0.001);
            midi.handle(off(1, 69));
            assert!((midi.frequency() - 246.9417).abs() < 0.001);
            midi.set_bend_range(12.0);
            assert_eq!(midi.frequency(), 440.0);
            midi.handle(off(0, 57));
            assert_eq!(midi.frequency(), 0.0);
        });
    }

    #[test]
    fn sustain_zero_velocity_and_channel_panic_do_not_leave_stuck_notes() {
        let mut midi = MidiTracker::default();
        midi.handle(on(0, 57));
        midi.handle(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: 64,
            value: 1.0,
        });
        midi.handle(on(0, 69));
        midi.handle(off(0, 69));
        assert_eq!(midi.frequency(), 440.0);
        midi.handle(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: 64,
            value: 0.0,
        });
        assert_eq!(midi.frequency(), 220.0);
        let mut zero = on(0, 57);
        if let NoteEvent::NoteOn { velocity, .. } = &mut zero {
            *velocity = 0.0;
        }
        midi.handle(zero);
        assert_eq!(midi.frequency(), 0.0);
        midi.handle(on(0, 57));
        midi.handle(on(1, 69));
        midi.handle(NoteEvent::MidiCC {
            timing: 0,
            channel: 1,
            cc: 120,
            value: 0.0,
        });
        assert_eq!(midi.frequency(), 220.0);
        midi.handle(NoteEvent::MidiCC {
            timing: 0,
            channel: 0,
            cc: 123,
            value: 0.0,
        });
        assert_eq!(midi.frequency(), 0.0);
    }

    #[test]
    fn voice_ids_and_wildcard_chokes_respect_overlapping_notes() {
        let mut midi = MidiTracker::default();
        for (id, tuning) in [(10, 0.0), (11, 12.0)] {
            let mut note = on(0, 69);
            if let NoteEvent::NoteOn { voice_id, .. } = &mut note {
                *voice_id = VoiceID::ID(id);
            }
            midi.handle(note);
            midi.handle(NoteEvent::PolyTuning {
                timing: 0,
                voice_id: VoiceID::ID(id),
                channel: Channel::Number(0),
                key: Key::Number(69),
                tuning,
            });
        }
        assert_eq!(midi.frequency(), 880.0);
        midi.handle(NoteEvent::Choke {
            timing: 0,
            voice_id: VoiceID::ID(11),
            channel: Channel::Wildcard,
            key: Key::Wildcard,
        });
        assert_eq!(midi.frequency(), 440.0);
        midi.handle(off(0, 69));
        assert_eq!(midi.frequency(), 0.0);
        midi.handle(on(0, 60));
        midi.handle(NoteEvent::Choke {
            timing: 0,
            voice_id: VoiceID::Wildcard,
            channel: Channel::Wildcard,
            key: Key::Wildcard,
        });
        assert_eq!(midi.frequency(), 0.0);
    }

    #[test]
    fn full_note_history_retriggers_and_resets_without_allocating() {
        let mut midi = MidiTracker::default();
        nice_assert_no_alloc::assert_no_alloc(|| {
            for channel in 0..16 {
                for key in 0..128 {
                    midi.handle(on(channel, key));
                }
            }
            assert_eq!(midi.len, 2048);
            midi.handle(on(0, 69));
            assert_eq!(midi.len, 2048);
            assert_eq!(midi.frequency(), 440.0);
            midi.handle(NoteEvent::MidiPitchBend {
                timing: 0,
                channel: 0,
                value: f32::NAN,
            });
            assert_eq!(midi.frequency(), 440.0);
            midi.reset();
            assert_eq!(midi.frequency(), 0.0);
            midi.handle(on(0, 69));
            assert_eq!(midi.frequency(), 440.0);
        });
    }
}
