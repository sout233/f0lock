use atomic_refcell::AtomicRefMut;
use clap_sys::events::clap_output_events;
use clap_sys::ext::remote_controls::{CLAP_REMOTE_CONTROLS_COUNT, clap_remote_controls_page};
use clap_sys::id::{CLAP_INVALID_ID, clap_id};
use clap_sys::string_sizes::CLAP_NAME_SIZE;
use nice_plug_core::context::PluginApi;
use nice_plug_core::context::activate::ActivateContext;
use nice_plug_core::context::process::{ProcessContext, SendEventError, Transport};
use nice_plug_core::context::remote_controls::{
    RemoteControlsContext, RemoteControlsPage, RemoteControlsSection,
};
use nice_plug_core::midi::{
    Channel, Key, MidiConfig, MidiResult, NoteEvent, PluginNoteEvent, VoiceID,
};
use nice_plug_core::params::Param;
use nice_plug_core::params::internals::ParamPtr;
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::mem;

use super::wrapper::{Task, Wrapper};
use crate::event_loop::EventLoop;
use crate::wrapper::clap::ClapPlugin;
use crate::wrapper::util::{clamp_output_event_timing, strlcpy};

/// An [`ActivateContext`] implementation for the wrapper.
///
/// # Note
///
/// See the VST3 `WrapperActivateContext` for an explanation of why we need this `pending_requests`
/// field.
pub(crate) struct WrapperActivateContext<'a, P: ClapPlugin> {
    pub(super) wrapper: &'a Wrapper<P>,
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
/// unnecessary atomic operations to lock the uncontested `RwLock`s.
pub(crate) struct WrapperProcessContext<'a, P: ClapPlugin> {
    pub(super) wrapper: &'a Wrapper<P>,
    pub(super) input_events_guard: AtomicRefMut<'a, VecDeque<PluginNoteEvent<P>>>,
    //pub(super) output_events_guard: AtomicRefMut<'a, VecDeque<PluginNoteEvent<P>>>,
    pub(super) transport: Transport,
    // used to clamp out of bounds events to the buffer's length.
    pub(super) total_buffer_len: u32,
    pub(super) current_sample_idx: u32,
    pub(super) host_out_events: *const clap_output_events,
}

/// A [`GuiContext`] implementation for the wrapper. This is passed to the plugin in
/// [`Editor::spawn()`][crate::prelude::Editor::spawn()] so it can interact with the rest of the plugin and
/// with the host for things like setting parameters.
#[cfg(feature = "editor")]
pub(crate) struct WrapperGuiContext<P: ClapPlugin> {
    pub(super) wrapper: std::sync::Weak<Wrapper<P>>,
    #[cfg(debug_assertions)]
    pub(super) param_gesture_checker:
        atomic_refcell::AtomicRefCell<crate::wrapper::util::context_checks::ParamGestureChecker>,
}

/// A [`RemoteControlsContext`] implementation for the wrapper. This is used during initialization
/// to allow the plugin to declare remote control pages. This struct defines the pages in the
/// correct format.
pub(crate) struct RemoteControlPages<'a> {
    param_ptr_to_hash: &'a HashMap<ParamPtr, u32>,
    /// The remote control pages, as defined by the plugin. These don't reference any heap data so
    /// we can store them directly.
    pages: &'a mut Vec<clap_remote_controls_page>,
}

impl<P: ClapPlugin> Drop for WrapperActivateContext<'_, P> {
    fn drop(&mut self) {
        if let Some(samples) = self.pending_requests.latency_changed.take() {
            self.wrapper.set_latency_samples(samples)
        }
    }
}

impl<P: ClapPlugin> ActivateContext<P> for WrapperActivateContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    fn execute(&self, task: P::BackgroundTask) {
        (self.wrapper.task_executor.lock())(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        // See this struct's docstring
        self.pending_requests.latency_changed.set(Some(samples));
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        self.wrapper.set_current_voice_capacity(capacity)
    }
}

impl<P: ClapPlugin> ProcessContext<P> for WrapperProcessContext<'_, P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    fn execute_background(&self, task: P::BackgroundTask) {
        let task_posted = self.wrapper.schedule_background(Task::PluginTask(task));
        crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    fn execute_gui(&self, task: P::BackgroundTask) {
        let task_posted = self.wrapper.schedule_gui(Task::PluginTask(task));
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
        use clap_sys::events::{
            CLAP_CORE_EVENT_SPACE_ID, CLAP_EVENT_MIDI, CLAP_EVENT_MIDI_SYSEX, CLAP_EVENT_NOTE_END,
            CLAP_EVENT_NOTE_EXPRESSION, CLAP_EVENT_NOTE_OFF, CLAP_EVENT_NOTE_ON,
            CLAP_NOTE_EXPRESSION_BRIGHTNESS, CLAP_NOTE_EXPRESSION_EXPRESSION,
            CLAP_NOTE_EXPRESSION_PAN, CLAP_NOTE_EXPRESSION_PRESSURE, CLAP_NOTE_EXPRESSION_TUNING,
            CLAP_NOTE_EXPRESSION_VIBRATO, CLAP_NOTE_EXPRESSION_VOLUME, clap_event_header,
            clap_event_midi, clap_event_midi_sysex, clap_event_note, clap_event_note_expression,
        };
        use nice_plug_core::midi::sysex::SysExMessage;
        use std::borrow::Borrow;

        if self.host_out_events.is_null() {
            return Err((event, SendEventError::NoOutputBuffer));
        }

        // Out of bounds events are clamped to the buffer's size
        let time = clamp_output_event_timing(
            event.timing() + self.current_sample_idx,
            self.total_buffer_len,
        );

        fn voice_to_i32(voice_id: VoiceID) -> i32 {
            voice_id.id().unwrap_or(-1)
        }
        fn channel_to_i16(channel: Channel) -> i16 {
            channel.number().map(|c| c as i16).unwrap_or(-1)
        }
        fn key_to_i16(key: Key) -> i16 {
            key.number().map(|k| k as i16).unwrap_or(-1)
        }

        let push_successful = match &event {
            NoteEvent::NoteOn {
                timing: _,
                voice_id,
                channel,
                key,
                velocity,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_ON,
                        // We don't have a way to denote live events
                        flags: 0,
                    },
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    velocity: *velocity as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::NoteOff {
                timing: _,
                voice_id,
                channel,
                key,
                velocity,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_OFF,
                        flags: 0,
                    },
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    velocity: *velocity as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            // NOTE: This is gated behind `P::MIDI_INPUT`, because this is a merely a hint event
            //       for the host. It is not output to any other plugin or device.
            NoteEvent::VoiceTerminated {
                timing: _,
                voice_id,
                channel,
                key,
            } if P::MIDI_INPUT >= MidiConfig::Basic => {
                let event = clap_event_note {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_END,
                        flags: 0,
                    },
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    velocity: 0.0,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyPressure {
                timing: _,
                voice_id,
                channel,
                key,
                pressure,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_PRESSURE,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *pressure as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyVolume {
                timing: _,
                voice_id,
                channel,
                key,
                gain,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_VOLUME,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *gain as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyPan {
                timing: _,
                voice_id,
                channel,
                key,
                pan,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_PAN,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: (*pan as f64 + 1.0) / 2.0,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyTuning {
                timing: _,
                voice_id,
                channel,
                key,
                tuning,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_TUNING,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *tuning as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyVibrato {
                timing: _,
                voice_id,
                channel,
                key,
                vibrato,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_VIBRATO,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *vibrato as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyExpression {
                timing: _,
                voice_id,
                channel,
                key,
                expression,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_EXPRESSION,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *expression as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::PolyBrightness {
                timing: _,
                voice_id,
                channel,
                key,
                brightness,
            } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                let event = clap_event_note_expression {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_note_expression>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_NOTE_EXPRESSION,
                        flags: 0,
                    },
                    expression_id: CLAP_NOTE_EXPRESSION_BRIGHTNESS,
                    note_id: voice_to_i32(*voice_id),
                    port_index: 0,
                    channel: channel_to_i16(*channel),
                    key: key_to_i16(*key),
                    value: *brightness as f64,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            midi_event @ (NoteEvent::MidiChannelPressure { .. }
            | NoteEvent::MidiPitchBend { .. }
            | NoteEvent::MidiCC { .. }
            | NoteEvent::MidiProgramChange { .. })
                if P::MIDI_OUTPUT >= MidiConfig::MidiCCs =>
            {
                // nice-plug already includes MIDI conversion functions, so we'll reuse those for
                // the MIDI events
                let midi_data = match midi_event.as_midi() {
                    Some(MidiResult::Basic(midi_data)) => midi_data,
                    Some(MidiResult::SysEx(_, _)) => unreachable!(
                        "Basic MIDI event read as SysEx, something's gone horribly wrong"
                    ),
                    None => unreachable!("Missing MIDI conversion for MIDI event"),
                };

                let event = clap_event_midi {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_midi>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_MIDI,
                        flags: 0,
                    },
                    port_index: 0,
                    data: midi_data,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
                }
            }
            NoteEvent::MidiSysEx { timing: _, message } if P::MIDI_OUTPUT >= MidiConfig::Basic => {
                // SysEx is supported on the basic MIDI config so this is separate
                let (padded_sysex_buffer, length) = message.as_buffer();
                let padded_sysex_buffer = padded_sysex_buffer.borrow();
                crate::nice_debug_assert!(padded_sysex_buffer.len() >= length);
                let sysex_buffer = &padded_sysex_buffer[..length];

                let event = clap_event_midi_sysex {
                    header: clap_event_header {
                        size: mem::size_of::<clap_event_midi_sysex>() as u32,
                        time,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_MIDI_SYSEX,
                        flags: 0,
                    },
                    port_index: 0,
                    // The host _should_ be making a copy of the data if it accepts the event. Should...
                    buffer: sysex_buffer.as_ptr(),
                    size: sysex_buffer.len() as u32,
                };

                unsafe {
                    clap_call! { self.host_out_events=>try_push(self.host_out_events, &event.header) }
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

        if push_successful {
            Ok(())
        } else {
            Err((event, SendEventError::HostBufferFull))
        }
    }

    fn set_latency_samples(&self, samples: u32) {
        self.wrapper.set_latency_samples(samples)
    }

    fn set_current_voice_capacity(&self, capacity: u32) {
        self.wrapper.set_current_voice_capacity(capacity)
    }

    fn request_restart(&self) {
        self.wrapper.request_restart();
    }
}

#[cfg(feature = "editor")]
impl<P: ClapPlugin> nice_plug_core::context::gui::GuiContextInner for WrapperGuiContext<P> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Clap
    }

    // All of these functions are supposed to be called from the main thread, so we'll put some
    // trust in the caller and assume that this is indeed the case
    unsafe fn raw_begin_set_parameter(&self, param: ParamPtr) {
        let wrapper = self.wrapper.upgrade().unwrap();

        match wrapper.param_ptr_to_hash.get(&param) {
            Some(hash) => {
                use crate::wrapper::clap::wrapper::OutputParamEvent;

                let success = wrapper
                    .queue_parameter_event(OutputParamEvent::BeginGesture { param_hash: *hash });

                crate::nice_debug_assert!(
                    success,
                    "Parameter output event queue was full, parameter change will not be sent to \
                     the host"
                );
            }
            None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
        }

        #[cfg(debug_assertions)]
        match wrapper.param_id_from_ptr(param) {
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
        let wrapper = self.wrapper.upgrade().unwrap();

        match wrapper.param_ptr_to_hash.get(&param) {
            Some(hash) => {
                use crate::wrapper::clap::wrapper::OutputParamEvent;

                // We queue the parameter change event here, and it will be sent to the host either
                // at the end of the current processing cycle or after requesting an explicit flush
                // (when the plugin isn't processing audio). The parameter's actual value will only
                // be changed when the output event is written to prevent changing parameter values
                // in the middle of processing audio.
                let clap_plain_value =
                    normalized as f64 * unsafe { param.step_count().unwrap_or(1) as f64 };
                let success = wrapper.queue_parameter_event(OutputParamEvent::SetValue {
                    param_hash: *hash,
                    clap_plain_value,
                });

                crate::nice_debug_assert!(
                    success,
                    "Parameter output event queue was full, parameter change will not be sent to \
                     the host"
                );
            }
            None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
        }

        #[cfg(debug_assertions)]
        match wrapper.param_id_from_ptr(param) {
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
        let wrapper = self.wrapper.upgrade().unwrap();

        match wrapper.param_ptr_to_hash.get(&param) {
            Some(hash) => {
                use crate::wrapper::clap::wrapper::OutputParamEvent;

                let success = wrapper
                    .queue_parameter_event(OutputParamEvent::EndGesture { param_hash: *hash });

                crate::nice_debug_assert!(
                    success,
                    "Parameter output event queue was full, parameter change will not be sent to \
                     the host"
                );
            }
            None => crate::nice_debug_assert_failure!("Unknown parameter: {:?}", param),
        }

        #[cfg(debug_assertions)]
        match wrapper.param_id_from_ptr(param) {
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

    fn get_state(&self) -> nice_plug_core::plugin::PluginState {
        self.wrapper.upgrade().unwrap().get_state_object()
    }

    fn set_state(&self, state: nice_plug_core::plugin::PluginState) {
        self.wrapper
            .upgrade()
            .unwrap()
            .set_state_object_from_gui(state)
    }

    fn request_restart(&self) {
        self.wrapper.upgrade().unwrap().request_restart();
    }
}

/// A remote control section. The plugin can fill this with information for one or more pages.
pub(crate) struct Section {
    pages: Vec<Page>,
}

/// A remote control page. These are automatically split into multiple pages if the number of
/// controls exceeds 8.
pub(crate) struct Page {
    name: String,
    params: Vec<Option<ParamPtr>>,
}

impl<'a> RemoteControlPages<'a> {
    /// Allow the plugin to define remote control pages and add them to `pages`. This does not clear
    /// `pages` first.
    pub fn define_remote_control_pages<P: ClapPlugin>(
        plugin: &P,
        pages: &'a mut Vec<clap_remote_controls_page>,
        param_ptr_to_hash: &'a HashMap<ParamPtr, u32>,
    ) {
        // The magic happens in the `add_section()` function defined below
        plugin.remote_controls(&mut Self {
            pages,
            param_ptr_to_hash,
        });
    }

    /// Perform the boilerplate needed for creating and adding a new [`clap_remote_controls_page`].
    /// If `params` contains more than eight parameters then any further parameters will be lost.
    fn add_clap_page(
        &mut self,
        section: &str,
        page_name: &str,
        params: impl IntoIterator<Item = Option<ParamPtr>>,
    ) {
        let mut page = clap_remote_controls_page {
            section_name: [0; CLAP_NAME_SIZE],
            // Pages are numbered sequentially
            page_id: self.pages.len() as clap_id,
            page_name: [0; CLAP_NAME_SIZE],
            param_ids: [CLAP_INVALID_ID; CLAP_REMOTE_CONTROLS_COUNT],
            is_for_preset: false,
        };
        strlcpy(&mut page.section_name, section);
        strlcpy(&mut page.page_name, page_name);

        let mut params = params.into_iter();
        for (param_id, param_ptr) in page.param_ids.iter_mut().zip(&mut params) {
            // `param_id` already has the correct value if `param_ptr` is empty/a spacer
            if let Some(param_ptr) = param_ptr {
                *param_id = self.param_ptr_to_id(param_ptr);
            }
        }

        crate::nice_debug_assert!(
            params.next().is_none(),
            "More than eight parameters were passed to 'RemoteControlPages::add_page()', this is \
             a nice-plug bug."
        );

        self.pages.push(page);
    }

    /// Transform a `ParamPtr` to the associated CLAP parameter ID/hash. Returns -1/invalid
    /// parameter and triggers a debug assertion when the parameter is not known.
    fn param_ptr_to_id(&self, ptr: ParamPtr) -> clap_id {
        match self.param_ptr_to_hash.get(&ptr) {
            Some(id) => *id,
            None => {
                crate::nice_debug_assert_failure!(
                    "An unknown parameter was added to a remote control page, ignoring..."
                );

                CLAP_INVALID_ID
            }
        }
    }
}

impl RemoteControlsContext for RemoteControlPages<'_> {
    type Section = Section;

    fn add_section(&mut self, name: impl Into<String>, f: impl FnOnce(&mut Self::Section)) {
        let section_name = name.into();
        let mut section = Section {
            pages: Vec::with_capacity(1),
        };
        f(&mut section);

        // The pages in the section may need to be split up into multiple pages if it defines more
        // than eight parameters. This keeps the interface flexible for potential future expansion
        // and makes manual paging unnecessary in some situations.
        for page in section.pages {
            if page.params.len() > CLAP_REMOTE_CONTROLS_COUNT {
                for (subpage_idx, subpage_params) in
                    page.params.chunks(CLAP_REMOTE_CONTROLS_COUNT).enumerate()
                {
                    let subpage_name = format!("{} {}", page.name, subpage_idx + 1);
                    self.add_clap_page(
                        &section_name,
                        &subpage_name,
                        subpage_params.iter().copied(),
                    );
                }
            } else {
                self.add_clap_page(&section_name, &page.name, page.params);
            }
        }
    }
}

impl RemoteControlsSection for Section {
    type Page = Page;

    fn add_page(&mut self, name: impl Into<String>, f: impl FnOnce(&mut Self::Page)) {
        let mut page = Page {
            name: name.into(),
            params: Vec::with_capacity(CLAP_REMOTE_CONTROLS_COUNT),
        };
        f(&mut page);

        self.pages.push(page);
    }
}

impl RemoteControlsPage for Page {
    fn add_param(&mut self, param: &impl Param) {
        self.params.push(Some(param.as_ptr()));
    }

    fn add_spacer(&mut self) {
        self.params.push(None);
    }
}
