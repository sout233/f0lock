use std::{
    error::Error,
    sync::{Arc, Mutex},
    time::Duration,
};

use iced_baseview::{PollSubNotifier, Program, baseview::HandlerError};
use nice_plug_core::{
    context::gui::GuiContext,
    editor::{Editor, HostMethods, ParentWindowHandle, ResizeHint, SpawnedEditor},
};

pub use iced_baseview as iced;

pub mod application;
#[doc(inline)]
pub use application::application;
pub use application::{Application, PersistentState};

mod editor;
pub use editor::{IcedEditorHandle, IcedEditorState, IcedNiceContext};

pub struct IcedEditor {
    inner: Box<dyn Editor<Handle = IcedEditorHandle>>,
}

impl Editor for IcedEditor {
    type Handle = IcedEditorHandle;

    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        wait_for_parent: bool,
        fallback_scale_factor: Option<f64>,
        gui_context: GuiContext,
        host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>> {
        self.inner.spawn(
            parent,
            wait_for_parent,
            fallback_scale_factor,
            gui_context,
            host,
        )
    }

    fn size(&self) -> nice_plug_core::editor::dpi::NativeSize<u32> {
        self.inner.size()
    }

    fn resize_hint(&self) -> ResizeHint {
        self.inner.resize_hint()
    }

    fn track_info_updated(&self, info: nice_plug_core::plugin::TrackInfo) {
        self.inner.track_info_updated(info);
    }

    unsafe fn assume_standalone_in_process(&self) {
        // Safety: This has the same invariants.
        unsafe {
            self.inner.assume_standalone_in_process();
        }
    }
}

/// Create a new `Editor` using the Iced GUI framework.
///
/// * `editor_state` - The initial state of the editor.
/// * `persistent_state` - Custom state which persists between editor opens.
/// * `notifier` - An atomic flag used to notify the program when it should
///   poll for new updates and redraw (i.e. as a result of the host updating
///   parameters or the audio thread updating the state of meters). This flag
///   is polled every frame right before drawing. If the flag is set then the
///   `poll_events` subscription will be called.
/// * `settings` - Additional settings for the editor.
/// * `build` - The function which builds the Iced program.
pub fn create_iced_editor<P, B, PState>(
    editor_state: Arc<IcedEditorState>,
    persistent_state: PState,
    notifier: PollSubNotifier,
    settings: IcedNiceSettings,
    build: B,
) -> Option<IcedEditor>
where
    P: Program + 'static,
    B: Fn(PersistentState<PState>, IcedNiceContext) -> Result<P, HandlerError>
        + 'static
        + Send
        + Sync,
    PState: Send + 'static,
{
    Some(IcedEditor {
        inner: Box::new(editor::IcedEditorInner {
            editor_state,
            persistent_state: Arc::new(Mutex::new(Some(persistent_state))),
            settings: Arc::new(settings),
            build: Arc::new(build),
            notifier,
        }),
    })
}

#[derive(Default, Debug, Clone, PartialEq)]
pub struct IcedNiceSettings {
    /// The window title
    pub title: String,

    /// Describes whether and how a host may resize an [`Editor`],
    /// returned from [`nice_plug_core::editor::Editor::resize_hint()`].
    ///
    /// The default is non-resizable (`can_resize: false`), matching the previous
    /// fixed-size behavior. To make an editor resizable, return a hint with
    /// `can_resize: true`; the per-axis flags and aspect-ratio fields refine how.
    pub resize_hint: ResizeHint,

    /// Ignore key inputs, except for modifier keys such as SHIFT and ALT
    ///
    /// By default this is set to `false`.
    pub ignore_non_modifier_keys: bool,

    /// Always redraw whenever the baseview window updates instead of only when iced wants to update
    /// the window. This works around a current baseview limitation where it does not support
    /// trigger a redraw on window visibility change (which may cause blank windows when opening or
    /// reopening the editor) and an iced limitation where it's not possible to have animations
    /// without using an asynchronous timer stream to send redraw messages to the application.
    ///
    /// By default this is set to `false`.
    pub always_redraw: bool,

    /// The rate at which the [`poll_events`](iced_baseview::poll_events) subscription will trigger.
    ///
    /// This subscription may trigger more often than this, but it will not trigger slower
    /// than this (or at least as long as the backend's timer is precise enough).
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    pub poll_interval: Duration,
}

impl IcedNiceSettings {
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Use the given window title
    #[inline]
    pub fn with_tile(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Describes whether and how a host may resize an [`Editor`], returned from
    /// [`nice_plug_core::editor::Editor::resize_hint()`].
    ///
    /// The default is non-resizable (`can_resize: false`), matching the previous
    /// fixed-size behavior. To make an editor resizable, return a hint with
    /// `can_resize: true`; the per-axis flags and aspect-ratio fields refine how.
    #[inline]
    pub fn with_resize_hint(mut self, resize_hint: ResizeHint) -> Self {
        self.resize_hint = resize_hint;
        self
    }

    /// Ignore key inputs, except for modifier keys such as SHIFT and ALT
    ///
    /// By default this is set to `false`.
    #[inline]
    pub fn with_ignore_non_modifier_keys(mut self, ignore: bool) -> Self {
        self.ignore_non_modifier_keys = ignore;
        self
    }

    /// Always redraw whenever the baseview window updates instead of only when iced wants to update
    /// the window. This works around a current baseview limitation where it does not support
    /// trigger a redraw on window visibility change (which may cause blank windows when opening or
    /// reopening the editor) and an iced limitation where it's not possible to have animations
    /// without using an asynchronous timer stream to send redraw messages to the application.
    ///
    /// By default this is set to `false`.
    #[inline]
    pub fn with_always_redraw(mut self, always_redraw: bool) -> Self {
        self.always_redraw = always_redraw;
        self
    }

    /// The rate at which the [`poll_events`](iced_baseview::poll_events) subscription will trigger.
    ///
    /// Defaults to `1.0 / 64.0` seconds.
    #[inline]
    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }
}
