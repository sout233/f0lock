use nice_plug_core::context::PluginApi;
use nice_plug_core::context::activate::ActivateContext;
use nice_plug_core::context::process::{ProcessContext, SendEventError, Transport};
use nice_plug_core::midi::PluginNoteEvent;
use nice_plug_core::plugin::Plugin;

#[cfg(feature = "editor")]
use nice_plug_core::{
    context::gui::GuiContextInner, params::internals::ParamPtr, plugin::PluginState,
};

use super::backend::Backend;
use super::wrapper::{Task, Wrapper};

/// An [`ActivateContext`] implementation for the standalone wrapper.
pub(crate) struct WrapperActivateContext<'a, P: Plugin, B: Backend<P>> {
    pub(super) wrapper: &'a Wrapper<P, B>,
}

/// A [`ProcessContext`] implementation for the standalone wrapper. This is a separate object so it
/// can hold on to lock guards for event queues. Otherwise reading these events would require
/// constant unnecessary atomic operations to lock the uncontested `RwLock`s.
pub(crate) struct WrapperProcessContext<'a, P: Plugin, B: Backend<P>> {
    #[allow(dead_code)]
    pub(super) wrapper: &'a Wrapper<P, B>,
    pub(super) input_events: &'a [PluginNoteEvent<P>],
    // The current index in `input_events`, since we're not actually popping anything from a queue
    // here to keep the standalone backend implementation a bit more flexible
    pub(super) input_events_idx: usize,
    pub(super) output_events: &'a mut Vec<PluginNoteEvent<P>>,
    pub(super) transport: Transport,
}

impl<P: Plugin, B: Backend<P>> ActivateContext<P> for WrapperActivateContext<'_, P, B> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
    }

    fn execute(&self, task: P::BackgroundTask) {
        (self.wrapper.task_executor.lock())(task);
    }

    fn set_latency_samples(&self, samples: u32) {
        self.wrapper.set_latency_samples(samples)
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }
}

impl<P: Plugin, B: Backend<P>> ProcessContext<P> for WrapperProcessContext<'_, P, B> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
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
        // We'll pretend we're a queue, choo choo
        if self.input_events_idx < self.input_events.len() {
            let event = self.input_events[self.input_events_idx].clone();
            self.input_events_idx += 1;

            Some(event)
        } else {
            None
        }
    }

    fn try_send_event(
        &mut self,
        event: PluginNoteEvent<P>,
    ) -> Result<(), (PluginNoteEvent<P>, SendEventError)> {
        if self.output_events.len() >= self.output_events.capacity() {
            return Err((event, SendEventError::HostBufferFull));
        }

        self.output_events.push(event);

        Ok(())
    }
    fn set_latency_samples(&self, samples: u32) {
        self.wrapper.set_latency_samples(samples)
    }

    fn set_current_voice_capacity(&self, _capacity: u32) {
        // This is only supported by CLAP
    }

    fn request_restart(&self) {
        // Not relevant for standalone backend
    }
}

/// A [`GuiContext`] implementation for the wrapper. This is passed to the plugin in
/// [`Editor::spawn()`][crate::prelude::Editor::spawn()] so it can interact with the rest of the plugin and
/// with the host for things like setting parameters.
#[cfg(feature = "editor")]
pub(crate) struct WrapperGuiContext<P: Plugin, B: Backend<P>> {
    pub(super) wrapper: std::sync::Weak<Wrapper<P, B>>,
    #[cfg(debug_assertions)]
    pub(super) param_gesture_checker:
        atomic_refcell::AtomicRefCell<crate::wrapper::util::context_checks::ParamGestureChecker>,
}

#[cfg(feature = "editor")]
impl<P: Plugin, B: Backend<P>> GuiContextInner for WrapperGuiContext<P, B> {
    fn plugin_api(&self) -> PluginApi {
        PluginApi::Standalone
    }

    unsafe fn raw_begin_set_parameter(&self, _param: ParamPtr) {
        // Since there's no automation being recorded here, gestures don't mean anything

        #[cfg(debug_assertions)]
        {
            let wrapper = self.wrapper.upgrade().unwrap();

            match wrapper.param_id_from_ptr(_param) {
                Some(param_id) => self
                    .param_gesture_checker
                    .borrow_mut()
                    .begin_set_parameter(param_id),
                None => crate::nice_debug_assert_failure!(
                    "raw_begin_set_parameter() called with an unknown ParamPtr"
                ),
            }
        }
    }

    unsafe fn raw_set_parameter_normalized(&self, param: ParamPtr, normalized: f32) {
        let wrapper = self.wrapper.upgrade().unwrap();

        wrapper.set_parameter(param, normalized);

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

    unsafe fn raw_end_set_parameter(&self, _param: ParamPtr) {
        #[cfg(debug_assertions)]
        {
            let wrapper = self.wrapper.upgrade().unwrap();

            match wrapper.param_id_from_ptr(_param) {
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
    }

    fn get_state(&self) -> PluginState {
        self.wrapper.upgrade().unwrap().get_state_object()
    }

    fn set_state(&self, state: PluginState) {
        self.wrapper
            .upgrade()
            .unwrap()
            .set_state_object_from_gui(state)
    }

    fn request_restart(&self) {
        // Not relevant for standalone backend
    }
}
