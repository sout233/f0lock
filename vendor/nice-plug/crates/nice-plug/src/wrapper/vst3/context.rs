use atomic_refcell::AtomicRefMut;
use nice_plug_core::{
    context::{
        PluginApi,
        activate::ActivateContext,
        process::{ProcessContext, SendEventError, Transport},
    },
    midi::{Channel, Key, MidiConfig, NoteEvent, PluginNoteEvent},
};
use std::collections::VecDeque;
use std::{cell::Cell, mem};
use vst3::{
    ComRef,
    Steinberg::{
        Vst::{
            DataEvent, Event, Event_::EventTypes_, IEventList, IEventListTrait,
            LegacyMIDICCOutEvent, NoteOffEvent, NoteOnEvent, PolyPressureEvent,
        },
        kResultOk,
    },
};

#[cfg(feature = "editor")]
use vst3::Steinberg::Vst::IComponentHandlerTrait;

#[cfg(feature = "editor")]
use nice_plug_core::{
    context::gui::GuiContextInner, params::internals::ParamPtr, plugin::PluginState,
};

use crate::wrapper::vst3::Vst3Plugin;
use crate::wrapper::{
    util::clamp_output_event_timing, vst3::note_expressions::NoteExpressionController,
};

use super::inner::{Task, WrapperInner};

/// An [`ActivateContext`] implementation for the wrapper.
///
/// # Note
///
/// Requests to change the latency are only sent when this object is dropped. Otherwise there's the
/// risk that the host will immediately deactivate/reactivate the plugin while still in the init
/// call. Reentrannt function calls are difficult to handle in Rust without forcing everything to
/// use interior mutability, so this will have to do for now. This does mean that `Plugin` mutex
/// lock has to be dropped before this object.
pub(crate) struct WrapperActivateContext<'a, P: Vst3Plugin> {
    pub(super) inner: &'a WrapperInner<P>,
    pub(super) pending_requests: PendingActivateContextRequests,
}

/// Any requests that should be sent out when the [`WrapperActivateContext`] is dropped. See that
/// struct's docstring for mroe information.
#[derive(Debug, Default)]
pub(crate) struct PendingActivateContextRequests {
    /// The value of the last `.set_latency_samples()` call.
    latency_changed: Cell<Option<u32>>,
}

/// A [`ProcessContext`] implementation for the wrapper. This is a separate object so it can hold on
/// to lock guards for event queues. Otherwise reading these events would require constant
/// unnecessary atomic operations to lock the uncontested locks.
pub(crate) struct WrapperProcessContext<'a, P: Vst3Plugin> {
    pub(super) inner: &'a WrapperInner<P>,
    pub(super) input_events_guard: AtomicRefMut<'a, VecDeque<PluginNoteEvent<P>>>,
    pub(super) transport: Transport,
    pub(super) host_out_events: Option<ComRef<'a, IEventList>>,
    // used to clamp out of bounds events to the buffer's length.
    pub(super) total_buffer_len: u32,
    pub(super) current_sample_idx: u32,
}

/// A [`GuiContext`] implementation for the wrapper. This is passed to the plugin in
/// [`Editor::spawn()`][crate::prelude::Editor::spawn()] so it can interact with the rest of the plugin and
/// with the host for things like setting parameters.
#[cfg(feature = "editor")]
pub(crate) struct WrapperGuiContext<P: Vst3Plugin> {
    pub(super) inner: std::sync::Weak<WrapperInner<P>>,
    #[cfg(debug_assertions)]
    pub(super) param_gesture_checker:
        atomic_refcell::AtomicRefCell<crate::wrapper::util::context_checks::ParamGestureChecker>,
}

impl<P: Vst3Plugin> Drop for WrapperActivateContext<'_, P> {
    fn drop(&mut self) {
        if let Some(samples) = self.pending_requests.latency_changed.take() {
            self.inner.set_latency_samples(samples)
        }
    }
}

impl<P: Vst3Plugin> ActivateContext<P> for WrapperActivateContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn execute(&self, task: P::BackgroundTask) {
        (self.inner.task_executor.lock())(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        // See this struct's docstring
        self.pending_requests.latency_changed.set(Some(samples));
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }
}

impl<P: Vst3Plugin> ProcessContext<P> for WrapperProcessContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    fn execute_background(&self, task: P::BackgroundTask) {
        let task_posted = self.inner.schedule_background(Task::PluginTask(task));
        crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    fn execute_gui(&self, task: P::BackgroundTask) {
        let task_posted = self.inner.schedule_gui(Task::PluginTask(task));
        crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    #[inline]
    fn transport(&self) -> &Transport {
        &self.transport
    }

    fn next_event(&mut self) -> Option<PluginNoteEvent<P>> {
        self.input_events_guard.pop_front()
    }

    fn try_send_event(
        &mut self,
        event: PluginNoteEvent<P>,
    ) -> Result<(), (PluginNoteEvent<P>, SendEventError)> {
        use nice_plug_core::midi::sysex::SysExMessage;
        use std::borrow::Borrow;

        let Some(host_out_events) = &mut self.host_out_events else {
            return Err((event, SendEventError::NoOutputBuffer));
        };

        // We'll set the correct variant on this struct, or skip to the next loop
        // iteration if we don't handle the event type
        let mut vst3_event: Event = unsafe { mem::zeroed() };
        vst3_event.busIndex = 0;
        // There's also a ppqPos field, but uh how about no
        vst3_event.sampleOffset = clamp_output_event_timing(
            event.timing() + self.current_sample_idx,
            self.total_buffer_len,
        ) as i32;

        fn channel_to_i16(channel: Channel) -> i16 {
            channel.number().map(|c| c as i16).unwrap_or(-1)
        }
        fn key_to_i16(key: Key) -> i16 {
            key.number().map(|k| k as i16).unwrap_or(-1)
        }

        // `voice_id.unwrap_or(|| ...)` triggers
        // https://github.com/rust-lang/rust-clippy/issues/8522
        #[allow(clippy::unnecessary_lazy_evaluations)]
        match &event {
            NoteEvent::NoteOn {
                timing: _,
                voice_id,
                channel,
                key,
                velocity,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                vst3_event.r#type = EventTypes_::kNoteOnEvent as u16;
                vst3_event.__field0.noteOn = NoteOnEvent {
                    channel: channel_to_i16(*channel),
                    pitch: key_to_i16(*key),
                    tuning: 0.0,
                    velocity: *velocity,
                    length: 0, // What?
                    // We'll use this for our note IDs, that way we don't have to do
                    // anything complicated here
                    noteId: voice_id.id_or_fallback(*key, *channel),
                };
            }
            NoteEvent::NoteOff {
                timing: _,
                voice_id,
                channel,
                key,
                velocity,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                vst3_event.r#type = EventTypes_::kNoteOffEvent as u16;
                vst3_event.__field0.noteOff = NoteOffEvent {
                    channel: channel_to_i16(*channel),
                    pitch: key_to_i16(*key),
                    velocity: *velocity,
                    noteId: voice_id.id_or_fallback(*key, *channel),
                    tuning: 0.0,
                };
            }
            // VST3 does not support or need these events, but they should also not
            // trigger a debug assertion failure in nice-plug. Also notes how this is
            // gated by `P::MIDI_INPUT`.
            NoteEvent::VoiceTerminated { .. } if P::MIDI_INPUT >= MidiConfig::Basic => {
                return Ok(());
            }
            NoteEvent::PolyPressure {
                timing: _,
                voice_id,
                channel,
                key,
                pressure,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                vst3_event.r#type = EventTypes_::kPolyPressureEvent as u16;
                vst3_event.__field0.polyPressure = PolyPressureEvent {
                    channel: channel_to_i16(*channel),
                    pitch: key_to_i16(*key),
                    noteId: voice_id.id_or_fallback(*key, *channel),
                    pressure: *pressure,
                };
            }
            event @ (NoteEvent::PolyVolume {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::PolyPan {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::PolyTuning {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::PolyVibrato {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::PolyExpression {
                voice_id,
                channel,
                key,
                ..
            }
            | NoteEvent::PolyBrightness {
                voice_id,
                channel,
                key,
                ..
            }) if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                match NoteExpressionController::translate_event_reverse(
                    voice_id.id_or_fallback(*key, *channel),
                    event,
                ) {
                    Some(translated_event) => {
                        vst3_event.r#type = EventTypes_::kNoteExpressionValueEvent as u16;
                        vst3_event.__field0.noteExpressionValue = translated_event;
                    }
                    None => {
                        crate::nice_debug_assert_failure!("Mishandled note expression value event");
                    }
                }
            }
            NoteEvent::MidiChannelPressure {
                timing: _,
                channel,
                pressure,
            } if P::MIDI_OUTPUT >= MidiConfig::MidiCCs => {
                vst3_event.r#type = EventTypes_::kLegacyMIDICCOutEvent as u16;
                vst3_event.__field0.midiCCOut = LegacyMIDICCOutEvent {
                    controlNumber: 128, // kAfterTouch
                    channel: *channel as std::ffi::c_char,
                    value: (pressure * 127.0).round() as std::ffi::c_char,
                    value2: 0,
                };
            }
            NoteEvent::MidiPitchBend {
                timing: _,
                channel,
                value,
            } if P::MIDI_OUTPUT >= MidiConfig::MidiCCs => {
                let scaled = (value * ((1 << 14) - 1) as f32).round() as i32;

                vst3_event.r#type = EventTypes_::kLegacyMIDICCOutEvent as u16;
                vst3_event.__field0.midiCCOut = LegacyMIDICCOutEvent {
                    controlNumber: 129, // kPitchBend
                    channel: *channel as std::ffi::c_char,
                    value: (scaled & 0b01111111) as std::ffi::c_char,
                    value2: ((scaled >> 7) & 0b01111111) as std::ffi::c_char,
                };
            }
            NoteEvent::MidiCC {
                timing: _,
                channel,
                cc,
                value,
            } if P::MIDI_OUTPUT >= MidiConfig::MidiCCs => {
                vst3_event.r#type = EventTypes_::kLegacyMIDICCOutEvent as u16;
                vst3_event.__field0.midiCCOut = LegacyMIDICCOutEvent {
                    controlNumber: *cc,
                    channel: *channel as std::ffi::c_char,
                    value: (value * 127.0).round() as std::ffi::c_char,
                    value2: 0,
                };
            }
            NoteEvent::MidiProgramChange {
                timing: _,
                channel,
                program,
            } if P::MIDI_OUTPUT >= MidiConfig::MidiCCs => {
                vst3_event.r#type = EventTypes_::kLegacyMIDICCOutEvent as u16;
                vst3_event.__field0.midiCCOut = LegacyMIDICCOutEvent {
                    controlNumber: 130, // kCtrlProgramChange
                    channel: *channel as std::ffi::c_char,
                    value: *program as std::ffi::c_char,
                    value2: 0,
                };
            }
            NoteEvent::MidiSysEx { timing: _, message } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let (padded_sysex_buffer, length) = message.as_buffer();
                let padded_sysex_buffer = padded_sysex_buffer.borrow();
                crate::nice_debug_assert!(padded_sysex_buffer.len() >= length);
                let sysex_buffer = &padded_sysex_buffer[..length];

                vst3_event.r#type = EventTypes_::kDataEvent as u16;
                vst3_event.__field0.data = DataEvent {
                    size: sysex_buffer.len() as u32,
                    r#type: 0, // kMidiSysEx
                    bytes: sysex_buffer.as_ptr(),
                };

                // NOTE: We need to have this call here while `sysex_buffer` is
                //       still in scope since the event contains pointers to it
                let result = unsafe { host_out_events.addEvent(&mut vst3_event) };
                if result == kResultOk {
                    return Ok(());
                } else {
                    return Err((event, SendEventError::HostBufferFull));
                }
            }
            _ => {
                return Err((
                    event,
                    SendEventError::InvalidEvent {
                        midi_output_config: P::MIDI_OUTPUT,
                    },
                ));
            }
        };

        let result = unsafe { host_out_events.addEvent(&mut vst3_event) };
        if result == kResultOk {
            Ok(())
        } else {
            Err((event, SendEventError::HostBufferFull))
        }
    }

    fn set_latency_samples(&self, samples: u32) {
        self.inner.set_latency_samples(samples)
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }

    fn request_restart(&self) {
        self.inner.request_restart();
    }
}

#[cfg(feature = "editor")]
impl<P: Vst3Plugin + Send> GuiContextInner for WrapperGuiContext<P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Vst3
    }

    // All of these functions are supposed to be called from the main thread, so we'll put some
    // trust in the caller and assume that this is indeed the case
    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        let inner = self.inner.upgrade().unwrap();

        match &*inner.component_handler.borrow() {
            Some(handler) => match inner.param_ptr_to_hash.get(&param) {
                Some(hash) => unsafe {
                    handler.beginEdit(*hash);
                },
                None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
            },
            None => crate::nice_debug_assert_failure!("Component handler not yet set"),
        }

        #[cfg(debug_assertions)]
        match inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .begin_set_parameter(param_id),
            None => crate::nice_debug_assert_failure!(
                "raw_begin_set_parameter() called with an unknown ParamPtr"
            ),
        }
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        let inner = self.inner.upgrade().unwrap();

        match &*inner.component_handler.borrow() {
            Some(handler) => match inner.param_ptr_to_hash.get(&param) {
                Some(hash) => {
                    // Only update the parameters manually if the host is not processing audio. If
                    // the plugin is currently processing audio, the host will pass this change back
                    // to the plugin in the audio callback. This also prevents the values from
                    // changing in the middle of the process callback, which would be unsound.
                    // FIXME: So this doesn't work for REAPER, because they just silently stop
                    //        processing audio when you bypass the plugin. Great. We can add a time
                    //        based heuristic to work around this in the meantime.
                    if !inner
                        .is_processing
                        .load(std::sync::atomic::Ordering::SeqCst)
                    {
                        inner.set_normalized_value_by_hash(
                            *hash,
                            normalized,
                            inner.current_buffer_config.load().map(|c| c.sample_rate),
                        );
                    }

                    unsafe { handler.performEdit(*hash, normalized as f64) };
                }
                None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
            },
            None => crate::nice_debug_assert_failure!("Component handler not yet set"),
        }

        #[cfg(debug_assertions)]
        match inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .set_parameter(param_id),
            None => {
                crate::nice_debug_assert_failure!(
                    "raw_set_parameter() called with an unknown ParamPtr"
                )
            }
        }
    }

    unsafe fn raw_end_set_parameter(&self, param: ParamPtr) {
        let inner = self.inner.upgrade().unwrap();

        match &*inner.component_handler.borrow() {
            Some(handler) => match inner.param_ptr_to_hash.get(&param) {
                Some(hash) => unsafe {
                    handler.endEdit(*hash);
                },
                None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
            },
            None => crate::nice_debug_assert_failure!("Component handler not yet set"),
        }

        #[cfg(debug_assertions)]
        match inner.param_id_from_ptr(param) {
            Some(param_id) => self
                .param_gesture_checker
                .borrow_mut()
                .end_set_parameter(param_id),
            None => {
                crate::nice_debug_assert_failure!(
                    "raw_end_set_parameter() called with an unknown ParamPtr"
                )
            }
        }
    }

    fn get_state(&self) -> PluginState {
        self.inner.upgrade().unwrap().get_state_object()
    }

    fn set_state(&self, state: PluginState) {
        self.inner
            .upgrade()
            .unwrap()
            .set_state_object_from_gui(state)
    }

    fn request_restart(&self) {
        self.inner.upgrade().unwrap().request_restart();
    }
}
