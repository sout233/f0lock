use atomic_refcell::AtomicRefCell;
use crossbeam::channel;
use crossbeam::queue::ArrayQueue;
use nice_plug_core::audio_setup::{AudioIOLayout, BufferConfig, ProcessMode};
#[cfg(feature = "editor")]
use nice_plug_core::context::gui::GuiContext;
use nice_plug_core::context::process::Transport;
#[cfg(feature = "editor")]
use nice_plug_core::editor::{Editor, EditorHandle, SpawnedEditor};
use nice_plug_core::midi::PluginNoteEvent;
use nice_plug_core::params::internals::ParamPtr;
use nice_plug_core::params::{ParamFlags, Params};
use nice_plug_core::plugin::{Plugin, PluginState, ProcessStatus, TaskExecutor};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
#[cfg(feature = "editor")]
use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::thread;

use super::backend::Backend;
use super::config::WrapperConfig;
use super::context::{WrapperActivateContext, WrapperProcessContext};
use crate::event_loop::{EventLoop, MainThreadExecutor, OsEventLoop};
use crate::util::permit_alloc;
#[cfg(feature = "editor")]
use crate::wrapper::standalone::context::WrapperGuiContext;
use crate::wrapper::state;
use crate::wrapper::util::process_wrapper;

/// How many parameter changes we can store in our unprocessed parameter change queue. Storing more
/// than this many parameters at a time will cause changes to get lost.
const EVENT_QUEUE_CAPACITY: usize = 2048;

pub struct Wrapper<P: Plugin, B: Backend<P>> {
    backend: AtomicRefCell<B>,

    /// The wrapped plugin instance.
    plugin: Mutex<P>,
    /// The plugin's background task executor closure. Tasks scheduled by the plugin will be
    /// executed on the GUI or background thread using this function.
    pub task_executor: Mutex<TaskExecutor<P>>,
    /// The plugin's parameters. These are fetched once during initialization. That way the
    /// `ParamPtr`s are guaranteed to live at least as long as this object and we can interact with
    /// the `Params` object without having to acquire a lock on `plugin`.
    params: Arc<dyn Params>,
    /// The plugin's editor, if it has one. This object does not do anything on its own, but we need
    /// to instantiate this in advance so we don't need to lock the entire [`Plugin`] object when
    /// creating an editor. Wrapped in an `AtomicRefCell` because it needs to be initialized late.
    #[allow(clippy::type_complexity)]
    #[cfg(feature = "editor")]
    pub editor: AtomicRefCell<Option<Arc<Mutex<P::Editor>>>>,
    #[allow(clippy::type_complexity)]
    #[cfg(feature = "editor")]
    pub editor_handle: Mutex<Option<<P::Editor as Editor>::Handle>>,
    close_requested: Arc<AtomicBool>,

    /// A realtime-safe task queue so the plugin can schedule tasks that need to be run later on the
    /// GUI thread. See the same field in the VST3 wrapper for more information on why this looks
    /// the way it does.
    event_loop: AtomicRefCell<Option<OsEventLoop<Task<P>, Self>>>,

    /// This is used to grab the DPI scaling config. Not used on macOS.
    #[allow(unused)]
    config: WrapperConfig,

    /// A mapping from parameter pointers to string parameter IDs. This is used as part of
    /// `Task::ParamValueChanged` to send a parameter change event to the editor from the GUI
    /// thread. This is also used to check whether the `ParamPtr` for an incoming parameter change
    /// actually belongs to a registered parameter.
    param_ptr_to_id: HashMap<ParamPtr, String>,
    /// A mapping from parameter string IDs to parameter pointers. Used for serialization and
    /// deserialization.
    param_id_to_ptr: HashMap<String, ParamPtr>,

    /// The bus and buffer configurations are static for the standalone target.
    audio_io_layout: AudioIOLayout,
    buffer_config: BufferConfig,

    /// Parameter changes that have been output by the GUI that have not yet been set in the plugin.
    /// This queue will be flushed at the end of every processing cycle, just like in the plugin
    /// versions.
    unprocessed_param_changes: ArrayQueue<(ParamPtr, f32)>,
    /// The plugin is able to restore state through a method on the `GuiContext`. To avoid changing
    /// parameters mid-processing and running into garbled data if the host also tries to load state
    /// at the same time the restoring happens at the end of each processing call. If this zero
    /// capacity channel contains state data at that point, then the audio thread will take the
    /// state out of the channel, restore the state, and then send it back through the same channel.
    /// In other words, the GUI thread acts as a sender and then as a receiver, while the audio
    /// thread acts as a receiver and then as a sender. That way deallocation can happen on the GUI
    /// thread. All of this happens without any blocking on the audio thread.
    updated_state_sender: channel::Sender<PluginState>,
    /// The receiver belonging to [`new_state_sender`][Self::new_state_sender].
    updated_state_receiver: channel::Receiver<PluginState>,
    /// The current latency in samples, as set by the plugin through the [`ActivateContext`] and the
    /// [`ProcessContext`]. This value may not be used depending on the audio backend, but it's
    /// still kept track of to avoid firing debug assertions multiple times for the same latency
    /// value.
    current_latency: AtomicU32,
}

/// Tasks that can be sent from the plugin to be executed on the main thread in a non-blocking
/// realtime-safe way (either a random thread or `IRunLoop` on Linux, the OS' message loop on
/// Windows and macOS).
#[allow(clippy::enum_variant_names)]
pub enum Task<P: Plugin> {
    /// Execute one of the plugin's background tasks.
    PluginTask(P::BackgroundTask),
    #[cfg(feature = "editor")]
    StateChanged,
    /// Inform the plugin that one parameter's value has changed. This uses the parameter hashes
    /// since the task will be created from the audio thread. We don't have parameter hashes here
    /// like in the plugin APIs, so we'll just use the `ParamPtr`s directly. These are used to index
    /// the hashmaps stored on `Wrapper`.
    #[cfg(feature = "editor")]
    ParameterValueChanged(ParamPtr, f32),
}

/// Errors that may arise while initializing the wrapped plugins.
#[derive(Debug, thiserror::Error)]
pub enum WrapperError {
    /// The plugin returned `false` during activation.
    #[error("The plugin returned false during activation")]
    ActivationFailed,
    #[cfg(feature = "editor")]
    #[error("{0}")]
    WindowError(#[from] Box<dyn Error>),
}

impl<P: Plugin, B: Backend<P>> MainThreadExecutor<Task<P>> for Wrapper<P, B> {
    fn execute(&self, task: Task<P>, _is_gui_thread: bool) {
        match task {
            Task::PluginTask(task) => (self.task_executor.lock())(task),
            #[cfg(feature = "editor")]
            Task::StateChanged => {
                if let Some(editor_handle) = self.editor_handle.lock().as_ref() {
                    editor_handle.state_changed();
                }
            }
            #[cfg(feature = "editor")]
            Task::ParameterValueChanged(param_ptr, normalized_value) => {
                if let Some(editor) = self.editor_handle.lock().as_ref() {
                    let param_id = &self.param_ptr_to_id[&param_ptr];
                    editor.param_value_changed(param_id, normalized_value);
                }
            }
        }
    }
}

impl<P: Plugin, B: Backend<P>> Wrapper<P, B> {
    /// Instantiate a new instance of the standalone wrapper. Returns an error if the plugin does
    /// not accept the IO configuration from the wrapper config.
    pub fn new(backend: B, config: WrapperConfig) -> Result<Arc<Self>, WrapperError> {
        // The backend has already queried this, so this will never cause the program to exit
        // TODO: Do the validation and parsing in the argument parser so this value can be stored on
        //       the config itself. Right now clap doesn't support this.
        let audio_io_layout = config.audio_io_layout_or_exit::<P>();

        let mut plugin = P::default();
        let task_executor = Mutex::new(plugin.task_executor());
        let params = plugin.params();

        // This is used to allow the plugin to restore preset data from its editor, see the comment
        // on `Self::updated_state_sender`
        let (updated_state_sender, updated_state_receiver) = channel::bounded(0);

        // For consistency's sake we'll include the same assertions as the other backends
        // TODO: Move these common checks to a function instead of repeating them in every wrapper
        let param_map = params.param_map();
        if cfg!(debug_assertions) {
            let param_ids: HashSet<_> = param_map.iter().map(|(id, _, _)| id.clone()).collect();
            crate::nice_debug_assert_eq!(
                param_map.len(),
                param_ids.len(),
                "The plugin has duplicate parameter IDs, weird things may happen. Consider using \
                 6 character parameter IDs to avoid collisions."
            );

            let mut bypass_param_exists = false;
            for (_, ptr, _) in &param_map {
                let flags = unsafe { ptr.flags() };
                let is_bypass = flags.contains(ParamFlags::BYPASS);

                if is_bypass && bypass_param_exists {
                    crate::nice_debug_assert_failure!(
                        "Duplicate bypass parameters found, the host will only use the first one"
                    );
                }

                bypass_param_exists |= is_bypass;
            }
        }

        let wrapper = Arc::new(Wrapper {
            backend: AtomicRefCell::new(backend),

            plugin: Mutex::new(plugin),
            task_executor,
            params,
            // Initialized later as it needs a reference to the wrapper for the async executor
            #[cfg(feature = "editor")]
            editor: AtomicRefCell::new(None),
            #[cfg(feature = "editor")]
            editor_handle: Mutex::new(None),
            close_requested: Arc::new(AtomicBool::new(false)),

            // Also initialized later as it also needs a reference to the wrapper
            event_loop: AtomicRefCell::new(None),

            param_ptr_to_id: param_map
                .iter()
                .map(|(param_id, param_ptr, _)| (*param_ptr, param_id.clone()))
                .collect(),
            param_id_to_ptr: param_map
                .into_iter()
                .map(|(param_id, param_ptr, _)| (param_id, param_ptr))
                .collect(),

            audio_io_layout,
            buffer_config: BufferConfig {
                sample_rate: config.sample_rate,
                min_buffer_size: None,
                max_buffer_size: config.period_size,
                // TODO: Detect JACK freewheeling and report it here
                process_mode: ProcessMode::Realtime,
            },
            config,

            unprocessed_param_changes: ArrayQueue::new(EVENT_QUEUE_CAPACITY),
            updated_state_sender,
            updated_state_receiver,
            current_latency: AtomicU32::new(0),
        });

        *wrapper.event_loop.borrow_mut() =
            Some(OsEventLoop::new_and_spawn(Arc::downgrade(&wrapper)));

        // The editor needs to be initialized later so the Async executor can work.
        #[cfg(feature = "editor")]
        {
            *wrapper.editor.borrow_mut() = wrapper
                .plugin
                .lock()
                .editor(nice_plug_core::context::gui::AsyncExecutor::new(
                    Arc::new({
                        let wrapper = wrapper.clone();

                        move |task| {
                            let task_posted = wrapper.schedule_background(Task::PluginTask(task));
                            crate::nice_debug_assert!(
                                task_posted,
                                "The task queue is full, dropping task..."
                            );
                        }
                    }),
                    Arc::new({
                        let wrapper = wrapper.clone();

                        move |task| {
                            let task_posted = wrapper.schedule_gui(Task::PluginTask(task));
                            crate::nice_debug_assert!(
                                task_posted,
                                "The task queue is full, dropping task..."
                            );
                        }
                    }),
                ))
                .map(|editor| {
                    // Safety: This is only called in standalone mode, and only if the user has
                    // enabled the `unsafe_standalone_dpi_fix` feature.
                    #[cfg(feature = "unsafe_standalone_dpi_fix")]
                    unsafe {
                        editor.assume_standalone_in_process();
                    }

                    Arc::new(Mutex::new(editor))
                });
        }

        // Before initializing the plugin, make sure all smoothers are set the the default values
        for param in wrapper.param_id_to_ptr.values() {
            unsafe { param._internal_update_smoother(wrapper.buffer_config.sample_rate, true) };
        }

        {
            let mut plugin = wrapper.plugin.lock();
            if !plugin.activate(
                &wrapper.audio_io_layout,
                &wrapper.buffer_config,
                &mut wrapper.make_activate_context(),
            ) {
                return Err(WrapperError::ActivationFailed);
            }
            process_wrapper(|| plugin.reset());
        }

        Ok(wrapper)
    }

    /// Open the editor, start processing audio, and block this thread until the editor is closed.
    /// If the plugin does not have an editor, then this will block until SIGINT is received.
    ///
    /// Will return an error if the plugin threw an error during audio processing or if the editor
    /// could not be opened.
    pub fn run(self: Arc<Self>) -> Result<(), WrapperError> {
        let close_gui_requested = Arc::clone(&self.close_requested);
        if let Err(e) = ctrlc::set_handler(move || {
            close_gui_requested.store(true, Ordering::Relaxed);
        }) {
            crate::nice_error!("Error setting close signal callback {}", e);
        }

        // We'll spawn a separate thread to handle IO and to process audio. This audio thread should
        // terminate together with this function.
        let terminate_audio_thread = Arc::new(AtomicBool::new(false));
        let close_gui_requested = Arc::clone(&self.close_requested);
        let audio_thread = {
            let this = self.clone();
            let terminate_audio_thread = terminate_audio_thread.clone();
            thread::spawn(move || {
                this.run_audio_thread(terminate_audio_thread, close_gui_requested)
            })
        };

        #[allow(unused_mut)]
        let mut gui_spawned = false;

        #[cfg(feature = "editor")]
        let res = match self.editor.borrow().clone() {
            Some(editor) => {
                let context = self.clone().make_gui_context();

                match editor.lock().spawn(None, false, None, context, None) {
                    Ok(editor_window) => {
                        let SpawnedEditor {
                            handle: instance,
                            window,
                        } = editor_window;

                        gui_spawned = true;
                        *self.editor_handle.lock() = Some(instance);

                        <P::Editor as Editor>::Handle::run_until_closed(window)
                            .map_err(|e| WrapperError::WindowError(e.into()))
                    }
                    Err(e) => Err(WrapperError::WindowError(e)),
                }
            }
            None => Ok(()),
        };

        #[cfg(not(feature = "editor"))]
        let res = Ok(());

        if !gui_spawned {
            crate::nice_log!("No GUI available for {}, blocking indefinitely...", P::NAME);

            loop {
                if self.close_requested.swap(false, Ordering::Relaxed) {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }

        terminate_audio_thread.store(true, Ordering::SeqCst);

        let _ = audio_thread.join();

        // Some plugins may use this to clean up resources. Should not be needed for the standalone
        // application, but it seems like a good idea to stay consistent.
        self.plugin.lock().deactivate();

        res
    }

    /// Get a parameter's ID based on a `ParamPtr`. Used in the `GuiContext` implementation for the
    /// gesture checks.
    #[allow(unused)]
    pub fn param_id_from_ptr(&self, param: ParamPtr) -> Option<&str> {
        self.param_ptr_to_id.get(&param).map(|s| s.as_str())
    }

    /// Set a parameter based on a `ParamPtr`. The value will be updated at the end of the next
    /// processing cycle, and this won't do anything if the parameter has not been registered by the
    /// plugin.
    ///
    /// This returns false if the parameter was not set because the `ParamPtr` was either unknown or
    /// the queue is full.
    #[cfg(feature = "editor")]
    pub fn set_parameter(&self, param: ParamPtr, normalized: f32) -> bool {
        if !self.param_ptr_to_id.contains_key(&param) {
            return false;
        }

        let push_successful = self
            .unprocessed_param_changes
            .push((param, normalized))
            .is_ok();
        crate::nice_debug_assert!(push_successful, "The parameter change queue was full");

        push_successful
    }

    /// Get the plugin's state object, may be called by the plugin's GUI as part of its own preset
    /// management. The wrapper doesn't use these functions and serializes and deserializes directly
    /// the JSON in the relevant plugin API methods instead.
    #[cfg(feature = "editor")]
    pub fn get_state_object(&self) -> PluginState {
        unsafe {
            state::serialize_object::<P>(
                self.params.clone(),
                self.param_id_to_ptr
                    .iter()
                    .map(|(param_id, param_ptr)| (param_id, *param_ptr)),
            )
        }
    }

    /// Update the plugin's internal state, called by the plugin itself from the GUI thread. To
    /// prevent corrupting data and changing parameters during processing the actual state is only
    /// updated at the end of the audio processing cycle.
    #[cfg(feature = "editor")]
    pub fn set_state_object_from_gui(&self, state: PluginState) {
        match self.updated_state_sender.send(state) {
            Ok(_) => {
                // As mentioned above, the state object will be passed back to this thread
                // so we can deallocate it without blocking.
                let state = self.updated_state_receiver.recv();
                drop(state);
            }
            Err(err) => {
                crate::nice_debug_assert_failure!(
                    "Could not send new state to the audio thread: {:?}",
                    err
                );
            }
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

    /// Posts the task to the task queue using [`EventLoop::schedule_gui()`] so it can be delegated
    /// to the main thread. The task is run directly if this is the GUI thread.
    ///
    /// If the task queue is full, then this will return false.
    #[must_use]
    pub fn schedule_gui(&self, task: Task<P>) -> bool {
        let event_loop = self.event_loop.borrow();
        let event_loop = event_loop.as_ref().unwrap();
        event_loop.schedule_gui(task)
    }

    pub fn set_latency_samples(&self, samples: u32) {
        // This should only change the value if it's actually needed
        let old_latency = self.current_latency.swap(samples, Ordering::SeqCst);
        if old_latency != samples {
            // None of the backends actually support this at the moment
            crate::nice_debug_assert_failure!(
                "Standalones currently don't support latency reporting"
            );
        }
    }

    /// The audio thread. This should be called from another thread, and it will run until
    /// `should_terminate` is `true`.
    fn run_audio_thread(
        self: Arc<Self>,
        should_terminate: Arc<AtomicBool>,
        close_gui_requested: Arc<AtomicBool>,
    ) {
        self.clone().backend.borrow_mut().run(
            move |buffer, aux, transport, input_events, output_events| {
                // TODO: This process wrapper should actually be in the backends (since the backends
                //       should also not allocate in their audio callbacks), but that's a bit more
                //       error prone
                process_wrapper(|| {
                    if should_terminate.load(Ordering::SeqCst) {
                        return false;
                    }

                    let sample_rate = self.buffer_config.sample_rate;
                    {
                        let mut plugin = self.plugin.lock();
                        if let ProcessStatus::Error(err) = plugin.process(
                            buffer,
                            aux,
                            &mut self.make_process_context(transport, input_events, output_events),
                        ) {
                            crate::nice_error!("The plugin returned an error while processing:");
                            crate::nice_error!("{}", err);

                            close_gui_requested.store(true, Ordering::Relaxed);

                            return false;
                        }
                    }

                    // Any output note events are now in a vector that can be processed by the
                    // audio/MIDI backend

                    // We'll always write these events to the first sample, so even when we add note
                    // output we shouldn't have to think about interleaving events here
                    while let Some((param_ptr, normalized_value)) =
                        self.unprocessed_param_changes.pop()
                    {
                        if unsafe { param_ptr._internal_set_normalized_value(normalized_value) } {
                            unsafe { param_ptr._internal_update_smoother(sample_rate, false) };

                            #[cfg(feature = "editor")]
                            {
                                let task_posted = self.schedule_gui(Task::ParameterValueChanged(
                                    param_ptr,
                                    normalized_value,
                                ));
                                crate::nice_debug_assert!(
                                    task_posted,
                                    "The task queue is full, dropping task..."
                                );
                            }
                        }
                    }

                    // After processing audio, we'll check if the editor has sent us updated plugin
                    // state.  We'll restore that here on the audio thread to prevent changing the
                    // values during the process call and also to prevent inconsistent state when
                    // the host also wants to load plugin state.
                    // FIXME: Zero capacity channels allocate on receiving, find a better
                    //        alternative that doesn't do that
                    let updated_state = permit_alloc(|| self.updated_state_receiver.try_recv());
                    if let Ok(mut state) = updated_state {
                        self.set_state_inner(&mut state);

                        // We'll pass the state object back to the GUI thread so deallocation can
                        // happen there without potentially blocking the audio thread
                        if let Err(err) = self.updated_state_sender.send(state) {
                            crate::nice_debug_assert_failure!(
                                "Failed to send state object back to GUI thread: {}",
                                err
                            );
                        };
                    }

                    true
                })
            },
        );
    }

    #[cfg(feature = "editor")]
    fn make_gui_context(self: Arc<Self>) -> GuiContext {
        GuiContext::new(Arc::new(WrapperGuiContext {
            wrapper: Arc::downgrade(&self),
            #[cfg(debug_assertions)]
            param_gesture_checker: Default::default(),
        }))
    }

    fn make_activate_context(&self) -> WrapperActivateContext<'_, P, B> {
        WrapperActivateContext { wrapper: self }
    }

    fn make_process_context<'a>(
        &'a self,
        transport: Transport,
        input_events: &'a [PluginNoteEvent<P>],
        output_events: &'a mut Vec<PluginNoteEvent<P>>,
    ) -> WrapperProcessContext<'a, P, B> {
        WrapperProcessContext {
            wrapper: self,
            input_events,
            input_events_idx: 0,
            output_events,
            transport,
        }
    }

    /// Immediately set the plugin state. Returns `false` if the deserialization failed. In other
    /// wrappers state is set from a couple places, so this function is here to be consistent and to
    /// centralize all of this behavior. Includes `permit_alloc()`s around the deserialization and
    /// initialization for the use case where `set_state_object_from_gui()` was called while the
    /// plugin is process audio.
    ///
    /// Implicitly emits `Task::ParameterValuesChanged`.
    ///
    /// # Notes
    ///
    /// `self.plugin` must _not_ be locked while calling this function or it will deadlock.
    fn set_state_inner(&self, state: &mut PluginState) -> bool {
        // FIXME: This is obviously not realtime-safe, but loading presets without doing this could
        //        lead to inconsistencies. It's the plugin's responsibility to not perform any
        //        realtime-unsafe work when the initialize function is called a second time if it
        //        supports runtime preset loading. `state::deserialize_object()` normally never
        //        allocates, but if the plugin has persistent non-parameter data then its
        //        `deserialize_fields()` implementation may still allocate.
        let mut success = permit_alloc(|| unsafe {
            state::deserialize_object::<P>(
                state,
                self.params.clone(),
                |param_id| self.param_id_to_ptr.get(param_id).copied(),
                Some(&self.buffer_config),
            )
        });
        if !success {
            crate::nice_debug_assert_failure!(
                "Deserializing plugin state from a state object failed"
            );
            return false;
        }

        // If the plugin was already initialized then it needs to be reinitialized
        {
            // NOTE: This needs to be dropped after the `plugin` lock to avoid deadlocks
            let mut activate_context = self.make_activate_context();
            let mut plugin = self.plugin.lock();

            // See above
            success = permit_alloc(|| {
                plugin.activate(
                    &self.audio_io_layout,
                    &self.buffer_config,
                    &mut activate_context,
                )
            });
            if success {
                process_wrapper(|| plugin.reset());
            }
        }

        crate::nice_debug_assert!(
            success,
            "Plugin returned false when reinitializing after loading state"
        );

        #[cfg(feature = "editor")]
        {
            // Reinitialize the plugin after loading state so it can respond to the new parameter values
            let task_posted = self.schedule_gui(Task::StateChanged);
            crate::nice_debug_assert!(task_posted, "The task queue is full, dropping task...");
        }

        success
    }
}
