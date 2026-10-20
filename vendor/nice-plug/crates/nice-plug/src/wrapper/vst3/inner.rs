use atomic_refcell::AtomicRefCell;
use crossbeam::atomic::AtomicCell;
use crossbeam::channel;
use nice_plug_core::audio_setup::{AudioIOLayout, BufferConfig, ProcessMode};
#[cfg(feature = "editor")]
use nice_plug_core::context::gui::GuiContext;
use nice_plug_core::context::process::Transport;
#[cfg(feature = "editor")]
use nice_plug_core::editor::dpi::Size;
#[cfg(feature = "editor")]
use nice_plug_core::editor::{Editor, SpawnedEditor};
use nice_plug_core::midi::{MidiConfig, PluginNoteEvent};
use nice_plug_core::params::internals::ParamPtr;
use nice_plug_core::params::{ParamFlags, Params};
#[cfg(feature = "editor")]
use nice_plug_core::plugin::TrackInfo;
use nice_plug_core::plugin::{Plugin, PluginState, ProcessStatus, TaskExecutor};
use parking_lot::Mutex;
#[cfg(feature = "editor")]
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use try_lock::TryLock;
#[cfg(feature = "editor")]
use vst3::ComWrapper;
use vst3::Steinberg::Vst::{IComponentHandler, IComponentHandlerTrait, IEventList, RestartFlags_};
use vst3::Steinberg::{kInvalidArgument, kResultOk, tresult};
use vst3::{ComPtr, ComRef};

use super::context::{WrapperActivateContext, WrapperProcessContext};
use super::note_expressions::NoteExpressionController;
use super::param_units::ParamUnits;
use super::util::{VST3_MIDI_PARAMS_END, VST3_MIDI_PARAMS_START};
use crate::event_loop::{EventLoop, MainThreadExecutor, OsEventLoop};
use crate::util::permit_alloc;
use crate::wrapper::state;
use crate::wrapper::util::buffer_management::BufferManager;
use crate::wrapper::util::hash_param_id;
use crate::wrapper::vst3::Vst3Plugin;
#[cfg(feature = "editor")]
use crate::wrapper::vst3::context::WrapperGuiContext;
#[cfg(feature = "editor")]
use crate::wrapper::vst3::view::WrapperView;

/// The actual wrapper bits. We need this as an `Arc<T>` so we can safely use our event loop API.
/// Since we can't combine that with VST3's interior reference counting this just has to be moved to
/// its own struct.
pub(crate) struct WrapperInner<P: Vst3Plugin> {
    /// The wrapped plugin instance.
    pub plugin: TryLock<P>,
    /// The plugin's background task executor closure.
    pub task_executor: Mutex<TaskExecutor<P>>,
    /// The plugin's parameters. These are fetched once during initialization. That way the
    /// `ParamPtr`s are guaranteed to live at least as long as this object and we can interact with
    /// the `Params` object without having to acquire a lock on `plugin`.
    pub params: Arc<dyn Params>,
    /// The plugin's editor, if it has one. This object does not do anything on its own, but we need
    /// to instantiate this in advance so we don't need to lock the entire [`Plugin`] object when
    /// creating an editor. Wrapped in an `AtomicRefCell` because it needs to be initialized late.
    #[allow(clippy::type_complexity)]
    #[cfg(feature = "editor")]
    pub editor: AtomicRefCell<Option<Arc<Mutex<P::Editor>>>>,
    #[allow(clippy::type_complexity)]
    // My personal record for craziest Rust type!!!!
    #[cfg(feature = "editor")]
    pub editor_window:
        Arc<AtomicRefCell<Option<fragile::Fragile<SpawnedEditor<<P::Editor as Editor>::Handle>>>>>,

    /// The host's [`IComponentHandler`] instance, if passed through
    /// [`IEditController::set_component_handler`].
    pub component_handler: AtomicRefCell<Option<ComPtr<IComponentHandler>>>,

    /// Our own [`IPlugView`] instance.
    #[cfg(feature = "editor")]
    pub plug_view: RwLock<Option<ComWrapper<WrapperView<P>>>>,

    /// Whether the editor is currently open. This is changed by the attached and removed functions on IPlugViewTrait
    #[cfg(feature = "editor")]
    pub is_editor_open: AtomicBool,

    /// A realtime-safe task queue so the plugin can schedule tasks that need to be run later on the
    /// GUI thread. This field should not be used directly for posting tasks. This should be done
    /// through [`Self::schedule_gui()`] instead. That method posts the task to the host's
    /// `IRunLoop` instead of it's available.
    ///
    /// This AtomicRefCell+Option is only needed because it has to be initialized late. There is no
    /// reason to mutably borrow the event loop, so reads will never be contested.
    ///
    /// TODO: Is there a better type for Send+Sync late initialization?
    pub event_loop: AtomicRefCell<Option<OsEventLoop<Task<P>, Self>>>,

    /// Whether the plugin is currently processing audio. In other words, the last state
    /// `IAudioProcessor::setActive()` has been called with.
    pub is_processing: AtomicBool,
    /// The current audio IO layout. Modified through `IAudioProcessor::setBusArrangements()` after
    /// matching the proposed bus arrangement to one of the supported ones. The plugin's first audio
    /// IO layout is chosen as the default. Because of the way VST3 works it's not possible to
    /// change the number of busses from that default, only the channel counts can change.
    pub current_audio_io_layout: AtomicCell<AudioIOLayout>,
    /// The current buffer configuration, containing the sample rate and the maximum block size.
    /// Will be set in `IAudioProcessor::setupProcessing()`.
    pub current_buffer_config: AtomicCell<Option<BufferConfig>>,
    /// The current audio processing mode. Set in `IAudioProcessor::setup_processing()`.
    pub current_process_mode: AtomicCell<ProcessMode>,
    /// The most recently reported track information. Hosts may send partial updates (e.g.
    /// Ableton), so this is used to merge successive [`IInfoListener::setChannelContextInfos()`]
    /// calls.
    #[cfg(feature = "editor")]
    pub current_track_info: AtomicRefCell<TrackInfo>,

    /// The last process status returned by the plugin. This is used for tail handling.
    pub last_process_status: AtomicCell<ProcessStatus>,
    /// The current latency in samples, as set by the plugin through the [`ActivateContext`] and the
    /// [`ProcessContext`].
    pub current_latency: AtomicU32,
    /// A data structure that helps manage and create buffers for all of the plugin's inputs and
    /// outputs based on channel pointers provided by the host.
    pub buffer_manager: AtomicRefCell<BufferManager>,
    /// The incoming events for the plugin, if `P::ACCEPTS_MIDI` is set. If
    /// `P::SAMPLE_ACCURATE_AUTOMATION`, this is also read in lockstep with the parameter change
    /// block splitting.
    ///
    /// NOTE: Because with VST3 MIDI CC messages are sent as parameter changes and VST3 does not
    ///       interleave parameter changes and note events, this queue has to be sorted when
    ///       creating the process context
    pub input_note_events: AtomicRefCell<VecDeque<PluginNoteEvent<P>>>,
    /// VST3 has several useful predefined note expressions, but for some reason they are the only
    /// note event type that don't have MIDI note ID and channel fields. So we need to keep track of
    /// the most recent VST3 note IDs we've seen, and then map those back to MIDI note IDs and
    /// channels as needed.
    pub note_expression_controller: AtomicRefCell<NoteExpressionController>,
    /// Unprocessed parameter changes and note events sent by the host during a process call.
    /// Parameter changes are sent as separate queues for each parameter, and note events are in
    /// another queue on top of that. And if `P::MIDI_INPUT >= MidiConfig::MidiCCs`, then we can
    /// also receive MIDI CC messages through special parameter changes. On top of that, we also
    /// support sample accurate automation through block splitting if
    /// `P::SAMPLE_ACCURATE_AUTOMATION` is set. To account for all of this, we'll read all of the
    /// parameter changes and events into a vector at the start of the process call, sort it, and
    /// then do the block splitting based on that. Note events need to have their timing adjusted to
    /// match the block start, since they're all read upfront.
    pub process_events: AtomicRefCell<Vec<ProcessEvent<P>>>,
    /// The plugin is able to restore state through a method on the `GuiContext`. To avoid changing
    /// parameters mid-processing and running into garbled data if the host also tries to load state
    /// at the same time the restoring happens at the end of each processing call. If this zero
    /// capacity channel contains state data at that point, then the audio thread will take the
    /// state out of the channel, restore the state, and then send it back through the same channel.
    /// In other words, the GUI thread acts as a sender and then as a receiver, while the audio
    /// thread acts as a receiver and then as a sender. That way deallocation can happen on the GUI
    /// thread. All of this happens without any blocking on the audio thread.
    pub updated_state_sender: channel::Sender<PluginState>,
    /// The receiver belonging to [`new_state_sender`][Self::new_state_sender].
    pub updated_state_receiver: channel::Receiver<PluginState>,

    /// The keys from `param_map` in a stable order.
    pub param_hashes: Vec<u32>,
    /// A mapping from parameter ID hashes (obtained from the string parameter IDs) to pointers to
    /// parameters belonging to the plugin. These addresses will remain stable as long as the
    /// `params` object does not get deallocated.
    pub param_by_hash: HashMap<u32, ParamPtr>,
    /// Mappings from parameter hashes to string parameter IDs. Used for notifying the plugin's
    /// editor about parameter changes.
    pub param_id_by_hash: HashMap<u32, String>,
    pub param_units: ParamUnits,
    /// Mappings from string parameter identifiers to parameter hashes. Useful for debug logging
    /// and when storing and restoring plugin state.
    pub param_id_to_hash: HashMap<String, u32>,
    /// The inverse mapping from [`param_by_hash`][Self::param_by_hash]. This is needed to be able
    /// to have an ergonomic parameter setting API that uses references to the parameters instead of
    /// having to add a setter function to the parameter (or even worse, have it be completely
    /// untyped).
    pub param_ptr_to_hash: HashMap<ParamPtr, u32>,
}

/// Tasks that can be sent from the plugin to be executed on the main thread in a non-blocking
/// realtime-safe way (either a random thread or `IRunLoop` on Linux, the OS' message loop on
/// Windows and macOS).
#[allow(clippy::enum_variant_names)]
pub enum Task<P: Plugin> {
    /// Execute one of the plugin's background tasks.
    PluginTask(P::BackgroundTask),
    /// Inform the plugin that one or more parameter values have changed.
    #[cfg(feature = "editor")]
    StateChanged,
    /// Inform the plugin that one parameter's value has changed. This uses the parameter hashes
    /// since the task will be created from the audio thread.
    #[cfg(feature = "editor")]
    ParameterValueChanged(u32, f32),
    /// Trigger a restart with the given restart flags. This is a bit set of the flags from
    /// [`vst3::Steinberg::Vst::RestartFlags`].
    TriggerRestart(i32),
    // Request the editor to be resized according to its current size. Right now there is no way to
    // handle "denied resize" requests yet.
    #[cfg(feature = "editor")]
    RequestResize { size: Size, scale_factor: f64 },
    #[cfg(feature = "editor")]
    CallMainThread,
}

/// VST3 makes audio processing pretty complicated. In order to support both block splitting for
/// sample accurate automation and MIDI CC handling through parameters we need to put all parameter
/// changes and (translated) note events into a sorted array first.
#[derive(Debug, PartialEq)]
pub enum ProcessEvent<P: Plugin> {
    /// An incoming parameter change sent by the host. This will only be used when sample accurate
    /// automation has been enabled, and the parameters are only updated when we process this
    /// spooled event at the start of a block.
    ParameterChange {
        /// The event's sample offset within the buffer. Used for sorting.
        timing: u32,
        /// The parameter's hash, as used everywhere else.
        hash: u32,
        /// The normalized values, as provided by the host.
        normalized_value: f32,
    },
    /// An incoming parameter change sent by the host. This will only be used when sample accurate
    /// automation has been enabled, and the parameters are only updated when we process this
    /// spooled event at the start of a block.
    ///
    /// The timing stored within the note event needs to have the block start index subtraced from
    /// it. make sure to subtract the block start index with [`NoteEvent::subtract_timing()`] before
    /// putting this into the input event queue.
    NoteEvent(PluginNoteEvent<P>),
}

impl<P: Plugin> ProcessEvent<P> {
    #[inline]
    pub(crate) fn timing(&self) -> u32 {
        match self {
            ProcessEvent::ParameterChange { timing, .. } => *timing,
            ProcessEvent::NoteEvent(event) => event.timing(),
        }
    }
}

impl<P: Vst3Plugin> WrapperInner<P> {
    #[allow(unused_unsafe)]
    pub fn new() -> Arc<Self> {
        let mut plugin = P::default();
        let task_executor = Mutex::new(plugin.task_executor());

        // This is used to allow the plugin to restore preset data from its editor, see the comment
        // on `Self::updated_state_sender`
        let (updated_state_sender, updated_state_receiver) = channel::bounded(0);

        // This is a mapping from the parameter IDs specified by the plugin to pointers to those
        // parameters. These pointers are assumed to be safe to dereference as long as
        // `wrapper.plugin` is alive. The plugin API identifiers these parameters by hashes, which
        // we'll calculate from the string ID specified by the plugin. These parameters should also
        // remain in the same order as the one returned by the plugin.
        let params = plugin.params();
        let param_id_hashes_ptrs_groups: Vec<_> = params
            .param_map()
            .into_iter()
            .map(|(id, ptr, group)| {
                let hash = hash_param_id(&id);
                (id, hash, ptr, group)
            })
            .collect();
        if cfg!(debug_assertions) {
            let param_map = params.param_map();
            let param_ids: HashSet<_> = param_id_hashes_ptrs_groups
                .iter()
                .map(|(id, _, _, _)| id.clone())
                .collect();
            crate::nice_debug_assert_eq!(
                param_map.len(),
                param_ids.len(),
                "The plugin has duplicate parameter IDs, weird things may happen. Consider using \
                 6 character parameter IDs to avoid collisions."
            );

            let mut bypass_param_exists = false;
            for (id, hash, ptr, _) in &param_id_hashes_ptrs_groups {
                let flags = unsafe { ptr.flags() };
                let is_bypass = flags.contains(ParamFlags::BYPASS);

                if is_bypass && bypass_param_exists {
                    crate::nice_debug_assert_failure!(
                        "Duplicate bypass parameters found, the host will only use the first one"
                    );
                }

                bypass_param_exists |= is_bypass;

                if P::MIDI_INPUT >= MidiConfig::MidiCCs
                    && (VST3_MIDI_PARAMS_START..VST3_MIDI_PARAMS_END).contains(hash)
                {
                    crate::nice_debug_assert_failure!(
                        "Parameter '{}' collides with an automatically generated MIDI CC \
                         parameter, consider giving it a different ID",
                        id
                    );
                }
            }
        }

        let param_hashes = param_id_hashes_ptrs_groups
            .iter()
            .map(|(_, hash, _, _)| *hash)
            .collect();
        let param_by_hash = param_id_hashes_ptrs_groups
            .iter()
            .map(|(_, hash, ptr, _)| (*hash, *ptr))
            .collect();
        let param_id_by_hash = param_id_hashes_ptrs_groups
            .iter()
            .map(|(id, hash, _, _)| (*hash, id.clone()))
            .collect();
        let param_units = ParamUnits::from_param_groups(
            param_id_hashes_ptrs_groups
                .iter()
                .map(|(_, hash, _, group_name)| (*hash, group_name.as_str())),
        )
        .expect("Inconsistent parameter groups");
        let param_id_to_hash = param_id_hashes_ptrs_groups
            .iter()
            .map(|(id, hash, _, _)| (id.clone(), *hash))
            .collect();
        let param_ptr_to_hash = param_id_hashes_ptrs_groups
            .into_iter()
            .map(|(_, hash, ptr, _)| (ptr, hash))
            .collect();

        let input_note_events = if P::MIDI_INPUT == MidiConfig::None {
            AtomicRefCell::new(VecDeque::new())
        } else {
            AtomicRefCell::new(VecDeque::with_capacity(P::INPUT_EVENT_CAPACITY))
        };

        let wrapper = Arc::new(Self {
            plugin: TryLock::new(plugin),
            task_executor,
            params,
            // Initialized later as it needs a reference to the wrapper for the async executor
            #[cfg(feature = "editor")]
            editor: AtomicRefCell::new(None),
            #[cfg(feature = "editor")]
            editor_window: Arc::new(AtomicRefCell::new(None)),

            component_handler: AtomicRefCell::new(None),

            #[cfg(feature = "editor")]
            plug_view: RwLock::new(None),

            #[cfg(feature = "editor")]
            is_editor_open: AtomicBool::new(false),

            event_loop: AtomicRefCell::new(None),

            is_processing: AtomicBool::new(false),
            // Some hosts, like the current version of Bitwig and Ardour at the time of writing,
            // will try using the plugin's default not yet initialized bus arrangement. Because of
            // that, we'll always initialize this configuration even before the host requests a
            // channel layout.
            current_audio_io_layout: AtomicCell::new(
                P::AUDIO_IO_LAYOUTS.first().copied().unwrap_or_default(),
            ),
            current_buffer_config: AtomicCell::new(None),
            current_process_mode: AtomicCell::new(ProcessMode::Realtime),
            #[cfg(feature = "editor")]
            current_track_info: AtomicRefCell::new(TrackInfo::default()),
            last_process_status: AtomicCell::new(ProcessStatus::Normal),
            current_latency: AtomicU32::new(0),
            // This is initialized just before calling `Plugin::activate()` so that during the
            // process call buffers can be initialized without any allocations
            buffer_manager: AtomicRefCell::new(BufferManager::for_audio_io_layout(
                0,
                AudioIOLayout::default(),
            )),
            input_note_events,
            note_expression_controller: AtomicRefCell::new(NoteExpressionController::default()),
            process_events: AtomicRefCell::new(Vec::with_capacity(P::INPUT_EVENT_CAPACITY)),
            updated_state_sender,
            updated_state_receiver,

            param_hashes,
            param_by_hash,
            param_id_by_hash,
            param_units,
            param_id_to_hash,
            param_ptr_to_hash,
        });

        // FIXME: Right now this is safe, but if we are going to have a singleton main thread queue
        //        serving multiple plugin instances, Arc can't be used because its reference count
        //        is separate from the internal COM-style reference count.
        *wrapper.event_loop.borrow_mut() =
            Some(OsEventLoop::new_and_spawn(Arc::downgrade(&wrapper)));

        // The editor also needs to be initialized later so the Async executor can work.

        #[cfg(feature = "editor")]
        {
            *wrapper.editor.borrow_mut() = wrapper
                .plugin
                .try_lock()
                .unwrap()
                .editor(nice_plug_core::context::gui::AsyncExecutor::new(
                    Arc::new({
                        let wrapper = Arc::downgrade(&wrapper);
                        move |task| {
                            let wrapper = match wrapper.upgrade() {
                                Some(wrapper) => wrapper,
                                None => return,
                            };

                            let task_posted = wrapper.schedule_background(Task::PluginTask(task));
                            crate::nice_debug_assert!(
                                task_posted,
                                "The task queue is full, dropping task..."
                            );
                        }
                    }),
                    Arc::new({
                        let wrapper = Arc::downgrade(&wrapper);
                        move |task| {
                            let wrapper = match wrapper.upgrade() {
                                Some(wrapper) => wrapper,
                                None => return,
                            };

                            let task_posted = wrapper.schedule_gui(Task::PluginTask(task));
                            crate::nice_debug_assert!(
                                task_posted,
                                "The task queue is full, dropping task..."
                            );
                        }
                    }),
                ))
                .map(|editor| Arc::new(Mutex::new(editor)));
        }

        wrapper
    }

    #[cfg(feature = "editor")]
    pub fn make_gui_context(self: Arc<Self>) -> GuiContext {
        GuiContext::new(Arc::new(WrapperGuiContext {
            inner: Arc::downgrade(&self),
            #[cfg(debug_assertions)]
            param_gesture_checker: Default::default(),
        }))
    }

    /// # Note
    ///
    /// The lock on the plugin must be dropped before this object is dropped to avoid deadlocks
    /// caused by reentrant function calls.
    pub fn make_activate_context(&self) -> WrapperActivateContext<'_, P> {
        WrapperActivateContext {
            inner: self,
            pending_requests: Default::default(),
        }
    }

    pub fn make_process_context<'a>(
        &'a self,
        transport: Transport,
        host_out_events: Option<ComRef<'a, IEventList>>,
        total_buffer_len: usize,
        current_sample_idx: usize,
    ) -> WrapperProcessContext<'a, P> {
        WrapperProcessContext {
            inner: self,
            input_events_guard: self.input_note_events.borrow_mut(),
            transport,
            host_out_events,
            total_buffer_len: total_buffer_len as u32,
            current_sample_idx: current_sample_idx as u32,
        }
    }

    /// Posts the task to the background task queue using [`EventLoop::schedule_background()`] so it
    /// can be run in the background without blocking either the GUI or the audio thread.
    ///
    /// If the task queue is full, then this will return false.
    #[must_use]
    pub fn schedule_background(&self, task: Task<P>) -> bool {
        let event_loop = self.event_loop.borrow();
        let event_loop = event_loop.as_ref().unwrap();
        event_loop.schedule_background(task)
    }

    /// Either posts the task to the task queue using [`EventLoop::schedule_gui()`] so it can be
    /// delegated to the main thread, executes the task directly if this is the main thread, or runs
    /// the task on the host's `IRunLoop` if the GUI is open and it exposes one.
    ///
    /// If the task queue is full, then this will return false.
    #[must_use]
    pub fn schedule_gui(&self, task: Task<P>) -> bool {
        let event_loop = self.event_loop.borrow();
        let event_loop = event_loop.as_ref().unwrap();
        if event_loop.is_main_thread() {
            self.execute(task, true);
            true
        } else {
            // If the editor is open, and the host exposes the `IRunLoop` interface, then we'll run
            // the task on the host's GUI thread using that interface. Otherwise we'll use the
            // regular event loop. If the editor gets dropped while there's still outstanding work
            // left in the run loop task queue, then those tasks will be posted to the regular event
            // loop so no work is lost.
            #[cfg(feature = "editor")]
            if self.is_editor_open.load(Ordering::Relaxed) {
                match self
                    .plug_view
                    .read()
                    .clone()
                    .unwrap()
                    .do_maybe_in_run_loop(task)
                {
                    Ok(()) => true,
                    Err(task) => event_loop.schedule_gui(task),
                }
            } else {
                event_loop.schedule_gui(task)
            }

            #[cfg(not(feature = "editor"))]
            event_loop.schedule_gui(task)
        }
    }

    /// Get a parameter's ID based on a `ParamPtr`. Used in the `GuiContext` implementation for the
    /// gesture checks.
    #[allow(unused)]
    pub fn param_id_from_ptr(&self, param: ParamPtr) -> Option<&str> {
        self.param_ptr_to_hash
            .get(&param)
            .and_then(|hash| self.param_id_by_hash.get(hash))
            .map(|s| s.as_str())
    }

    /// Convenience function for setting a value for a parameter as triggered by a VST3 parameter
    /// update. The same rate is for updating parameter smoothing.
    ///
    /// After calling this function, you should call
    /// [`notify_param_values_changed()`][Self::notify_param_values_changed()] to allow the editor
    /// to update itself. This needs to be done separately so you can process parameter changes in
    /// batches.
    pub fn set_normalized_value_by_hash(
        &self,
        hash: u32,
        normalized_value: f32,
        sample_rate: Option<f32>,
    ) -> tresult {
        if !normalized_value.is_finite() {
            return kInvalidArgument;
        }

        match self.param_by_hash.get(&hash) {
            Some(param_ptr) => {
                if unsafe { param_ptr._internal_set_normalized_value(normalized_value) } {
                    if let Some(sample_rate) = sample_rate {
                        unsafe { param_ptr._internal_update_smoother(sample_rate, false) };
                    }

                    #[cfg(feature = "editor")]
                    {
                        let task_posted =
                            self.schedule_gui(Task::ParameterValueChanged(hash, normalized_value));
                        crate::nice_debug_assert!(
                            task_posted,
                            "The task queue is full, dropping task..."
                        );
                    }
                }

                kResultOk
            }
            _ => kInvalidArgument,
        }
    }

    /// Get the plugin's state object, may be called by the plugin's GUI as part of its own preset
    /// management. The wrapper doesn't use these functions and serializes and deserializes directly
    /// the JSON in the relevant plugin API methods instead.
    #[cfg(feature = "editor")]
    pub fn get_state_object(&self) -> PluginState {
        unsafe {
            state::serialize_object::<P>(
                self.params.clone(),
                state::make_params_iter(&self.param_by_hash, &self.param_id_to_hash),
            )
        }
    }

    /// Update the plugin's internal state, called by the plugin itself from the GUI thread. To
    /// prevent corrupting data and changing parameters during processing the actual state is only
    /// updated at the end of the audio processing cycle.
    #[cfg(feature = "editor")]
    pub fn set_state_object_from_gui(&self, mut state: PluginState) {
        use crossbeam::channel::SendTimeoutError;

        // Use a loop and timeouts to handle the super rare edge case when this function gets called
        // between a process call and the host disabling the plugin
        loop {
            if self.is_processing.load(Ordering::SeqCst) {
                // If the plugin is currently processing audio, then we'll perform the restore
                // operation at the end of the audio call. This involves sending the state to the
                // audio thread, having the audio thread handle the state restore at the very end of
                // the process function, and then sending the state back to this thread so it can be
                // deallocated without blocking the audio thread.
                match self
                    .updated_state_sender
                    .send_timeout(state, std::time::Duration::from_secs(1))
                {
                    Ok(_) => {
                        // As mentioned above, the state object will be passed back to this thread
                        // so we can deallocate it without blocking.
                        let state = self.updated_state_receiver.recv();
                        drop(state);
                        break;
                    }
                    Err(SendTimeoutError::Timeout(value)) => {
                        state = value;
                        continue;
                    }
                    Err(SendTimeoutError::Disconnected(_)) => {
                        crate::nice_debug_assert_failure!("State update channel got disconnected");
                        return;
                    }
                }
            } else {
                // Otherwise we'll set the state right here and now, since this function should be
                // called from a GUI thread
                self.set_state_inner(&mut state);
                break;
            }
        }

        // After the state has been updated, notify the host about the new parameter values
        let task_posted = self
            .event_loop
            .borrow()
            .as_ref()
            .unwrap()
            .schedule_gui(Task::TriggerRestart(RestartFlags_::kParamValuesChanged));
        crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    pub fn set_latency_samples(&self, samples: u32) {
        // Only trigger a restart if it's actually needed
        let old_latency = self.current_latency.swap(samples, Ordering::SeqCst);
        if old_latency != samples {
            self.request_restart();
        }
    }

    pub fn request_restart(&self) {
        let task_posted = self.schedule_gui(Task::TriggerRestart(RestartFlags_::kLatencyChanged));
        crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
    }

    /// Immediately set the plugin state. Returns `false` if the deserialization failed. The plugin
    /// state is set from a couple places, so this function aims to deduplicate that. Includes
    /// `permit_alloc()`s around the deserialization and initialization for the use case where
    /// `set_state_object_from_gui()` was called while the plugin is process audio.
    ///
    /// Implicitly emits `Task::ParameterValuesChanged`.
    ///
    /// # Notes
    ///
    /// `self.plugin` must _not_ be locked while calling this function or it will deadlock.
    pub fn set_state_inner(&self, state: &mut PluginState) -> bool {
        // FIXME: This is obviously not realtime-safe, but loading presets without doing this
        //        could lead to inconsistencies. `state::deserialize_object()` normally never
        //        allocates, but if the plugin has persistent non-parameter data then its
        //        `deserialize_fields()` implementation may still allocate.
        let success = permit_alloc(|| unsafe {
            state::deserialize_object::<P>(
                state,
                self.params.clone(),
                state::make_params_getter(&self.param_by_hash, &self.param_id_to_hash),
                self.current_buffer_config.load().as_ref(),
            )
        });
        if !success {
            crate::nice_debug_assert_failure!(
                "Deserializing plugin state from a state object failed"
            );
            return false;
        }

        #[cfg(feature = "editor")]
        {
            // Reinitialize the plugin after loading state so it can respond to the new parameter values
            let task_posted = self.schedule_gui(Task::StateChanged);
            crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
        }

        success
    }
}

impl<P: Vst3Plugin> MainThreadExecutor<Task<P>> for WrapperInner<P> {
    fn execute(&self, task: Task<P>, is_gui_thread: bool) {
        // This function is always called from the main thread
        match task {
            Task::PluginTask(task) => (self.task_executor.lock())(task),
            #[cfg(feature = "editor")]
            Task::StateChanged => {
                use nice_plug_core::editor::EditorHandle;

                if let Some(editor_window) = self.editor_window.borrow().as_ref() {
                    editor_window.get().handle.state_changed();
                }
            }
            #[cfg(feature = "editor")]
            Task::ParameterValueChanged(param_hash, normalized_value) => {
                use nice_plug_core::editor::EditorHandle;

                if let Some(editor_window) = self.editor_window.borrow().as_ref() {
                    let param_id = &self.param_id_by_hash[&param_hash];
                    editor_window
                        .get()
                        .handle
                        .param_value_changed(param_id, normalized_value);
                }
            }
            Task::TriggerRestart(flags) => match &*self.component_handler.borrow() {
                Some(handler) => unsafe {
                    crate::nice_debug_assert!(is_gui_thread);
                    let result = handler.restartComponent(flags);
                    crate::nice_debug_assert_eq!(
                        result,
                        kResultOk,
                        "Failed the restart request call with flags '{:?}'",
                        flags
                    );
                },
                None => crate::nice_debug_assert_failure!("Component handler not yet set"),
            },
            #[cfg(feature = "editor")]
            Task::RequestResize { size, scale_factor } => {
                if self.is_editor_open.load(Ordering::SeqCst) {
                    unsafe {
                        crate::nice_debug_assert!(is_gui_thread);
                        let _success = WrapperView::request_resize(
                            &self.plug_view.read().clone().unwrap(),
                            size,
                            scale_factor,
                        );
                    }
                } else {
                    crate::nice_debug_assert_failure!("Can't resize a closed editor");
                }
            }
            #[cfg(feature = "editor")]
            Task::CallMainThread => {
                use nice_plug_core::editor::EditorHandle;

                if let Some(editor_window) = self.editor_window.borrow().as_ref() {
                    let editor_window = editor_window.get();
                    editor_window
                        .handle
                        .host_main_thread_callback(&editor_window.window)
                }
            }
        }
    }
}
