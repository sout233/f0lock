use crossbeam_utils::atomic::AtomicCell;
use iced_baseview::baseview::HandlerError;
use iced_baseview::shell::SharedWindowSize;
use iced_baseview::{IcedWindowSettings, PollSubNotifier, Program, baseview, message};
use nice_plug_core::context::gui::GuiContext;
use nice_plug_core::editor::dpi::{LogicalSize, NativeSize, PhysicalSize, Size};
use nice_plug_core::editor::{
    Editor, EditorHandle, HostMethods, Modifiers, ParentWindowHandle, ResizeHint, SizeConstraints,
    SpawnedEditor, VirtualKeyCode,
};
use nice_plug_core::plugin::TrackInfo;
use std::error::Error;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::{IcedNiceSettings, application::PersistentState};

pub(crate) struct IcedEditorInner<P: Program + 'static, PState: Send + 'static>
where
    <P as Program>::Message: message::MaybeDebug + message::MaybeClone,
{
    pub(crate) editor_state: Arc<IcedEditorState>,
    pub(crate) persistent_state: Arc<Mutex<Option<PState>>>,

    /// The user's build function. Applied once at the start of the application.
    pub(crate) build: Arc<
        dyn Fn(PersistentState<PState>, IcedNiceContext) -> Result<P, HandlerError>
            + 'static
            + Send
            + Sync,
    >,
    pub(crate) notifier: PollSubNotifier,

    pub(crate) settings: Arc<IcedNiceSettings>,
}

impl<P: Program + 'static, State: Send + 'static> Editor for IcedEditorInner<P, State> {
    type Handle = IcedEditorHandle;

    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        wait_for_parent: bool,
        fallback_scale_factor: Option<f64>,
        gui_context: GuiContext,
        host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>> {
        let build = self.build.clone();
        let persistent_state = PersistentState::from_shared(&self.persistent_state);
        let size = self.editor_state.size.load();
        let zoom_factor = self.editor_state.zoom_factor.load();
        let nice_context = gui_context.clone();
        let editor_state = self.editor_state.clone();

        let (min_size, max_size): (Option<Size>, Option<Size>) =
            match self.settings.resize_hint.size_constraints {
                SizeConstraints::Logical { min_size, max_size } => {
                    (min_size.map(|s| s.into()), max_size.map(|s| s.into()))
                }
                SizeConstraints::Physical { min_size, max_size } => {
                    (min_size.map(|s| s.into()), max_size.map(|s| s.into()))
                }
            };

        let settings = IcedWindowSettings::new()
            .with_title(self.settings.title.clone())
            .with_size(size)
            .with_min_size(min_size)
            .with_max_size(max_size)
            .with_resizable(self.settings.resize_hint.can_resize)
            .with_scale_factor(zoom_factor)
            .with_parent(parent.as_ref())
            .with_wait_for_parent(wait_for_parent)
            .with_fallback_scale_factor(fallback_scale_factor)
            .with_ignore_non_modifier_keys(self.settings.ignore_non_modifier_keys)
            .with_always_redraw(self.settings.always_redraw)
            .with_poll_interval(self.settings.poll_interval);

        // The host is a re-implementation of
        // [`baseview::host::HostCallbacks`](https://docs.rs/baseview/latest/baseview/host/trait.HostCallbacks.html)
        // and
        // [`baseview::host::HostMainThreadCaller`](https://docs.rs/baseview/latest/baseview/host/trait.HostMainThreadCaller.html)
        // to avoid `nice-plug-core` from depending on `baseview` until it is stabilized.
        //
        // Create a small wrapper to adapt it to baseview's HostCallbacks traits.
        let host = {
            struct HostCallbackAdapter {
                host: Box<dyn nice_plug_core::editor::HostCallbacks>,
            }
            impl baseview::host::HostCallbacks for HostCallbackAdapter {
                fn request_resize(
                    &mut self,
                    new_size: baseview::WindowSize,
                ) -> Result<(), HandlerError> {
                    self.host
                        .request_resize(new_size.physical.into(), new_size.scale_factor)
                        .map_err(HandlerError::from_boxed)
                }

                fn destroyed(&mut self) {
                    self.host.destroyed();
                }
            }
            struct HostMainThreadCallerAdapter {
                host: Box<dyn nice_plug_core::editor::HostMainThreadCaller>,
            }
            impl baseview::host::HostMainThreadCaller for HostMainThreadCallerAdapter {
                fn call_main_thread(&mut self) {
                    self.host.call_main_thread();
                }
            }
            host.map(|host| {
                baseview::host::Host::new()
                    .with_callbacks(HostCallbackAdapter {
                        host: host.callbacks,
                    })
                    .with_main_thread(HostMainThreadCallerAdapter {
                        host: host.main_thread_caller,
                    })
            })
        };

        let (window, _message_sender) = iced_baseview::create_window(
            settings,
            self.notifier.clone(),
            move |shared_window_size| {
                let nice_ctx = IcedNiceContext {
                    nice_context,
                    shared_window_size,
                    editor_state,
                };

                (build)(persistent_state, nice_ctx)
            },
            host,
        )?;

        self.editor_state.open.store(true, Ordering::Release);

        Ok(SpawnedEditor {
            handle: IcedEditorHandle {
                editor_state: Arc::clone(&self.editor_state),
                notifier: self.notifier.clone(),
                resize_hint: self.settings.resize_hint,
            },
            window,
        })
    }

    fn size(&self) -> NativeSize<u32> {
        NativeSize::from_size(
            self.editor_state.logical_size().into(),
            self.editor_state.system_scale_factor() as f64,
        )
    }

    fn resize_hint(&self) -> nice_plug_core::editor::ResizeHint {
        self.settings.resize_hint
    }

    fn track_info_updated(&self, info: nice_plug_core::plugin::TrackInfo) {
        {
            *self.editor_state.track_info.lock().unwrap() = info;
        }
        self.editor_state
            .track_info_changed
            .store(true, Ordering::Release);

        self.notifier.notify();
    }

    unsafe fn assume_standalone_in_process(&self) {
        // Safety: Editor::assume_standalone_in_process() has the same invariants as
        // baseview::assume_standalone_in_process()
        unsafe {
            baseview::assume_standalone_in_process();
        }
    }
}

/// The window handle used for [`IcedEditor`](crate::IcedEditor).
pub struct IcedEditorHandle {
    editor_state: Arc<IcedEditorState>,
    notifier: PollSubNotifier,
    resize_hint: ResizeHint,
}

impl EditorHandle for IcedEditorHandle {
    type Window = iced_baseview::baseview::Window;
    type Error = iced_baseview::baseview::Error;

    fn run_until_closed(window: Self::Window) -> Result<(), Self::Error> {
        window.run_until_closed()
    }

    fn set_parent(
        &self,
        parent: ParentWindowHandle,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        window.set_parent(&parent)
    }

    fn show(&self, window: &Self::Window) -> Result<(), Self::Error> {
        let res = window.show();

        // TODO: This might be a baseview bug. the `baseview::WindowEvent::Focused`
        // event does not always get called after `window.show()` is called. Either
        // that, or baseview needs to add a `baseview::WindowEvent::Shown` event.
        //
        // For now, we must manually trigger a redraw.
        self.notifier.notify();

        res
    }

    fn hide(&self, window: &Self::Window) -> Result<(), Self::Error> {
        window.hide()
    }

    fn host_main_thread_callback(&self, window: &Self::Window) {
        window.host_main_thread_callback();
    }

    fn set_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        window.resize(new_size)
    }

    fn set_fallback_scale_factor(
        &self,
        scale_factor: f64,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        window.suggest_fallback_scale_factor(scale_factor)
    }

    /// Return the closest supported size.
    fn adjust_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Option<NativeSize<u32>> {
        let current_size = window.size();
        Some(self.resize_hint.adjust_size(
            new_size,
            NativeSize::from_logical_or_physical(current_size.logical, current_size.physical),
            current_size.scale_factor,
        ))
    }

    fn on_virtual_key_from_host(
        &self,
        _key_code: VirtualKeyCode,
        _is_down: bool,
        _modifiers: Modifiers,
    ) -> bool {
        // TODO
        false
    }

    fn state_changed(&self) {
        self.notifier.notify();
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {
        self.notifier.notify();
    }

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {
        self.notifier.notify();
    }
}

impl Drop for IcedEditorHandle {
    fn drop(&mut self) {
        self.editor_state.open.store(false, Ordering::Release);
    }
}

/// State for an `nice-plug-iced` editor window.
#[derive(Debug)]
pub struct IcedEditorState {
    size: AtomicCell<Size>,

    pub(crate) zoom_factor: AtomicCell<f32>,

    /// The scaling factor reported by the host, if any. On macOS this will never be set and we
    /// should use the system scaling factor instead.
    pub(crate) fallback_scale_factor: AtomicCell<Option<f32>>,

    pub(crate) system_scale_factor: AtomicCell<Option<f32>>,

    /// Whether the editor's window is currently open.
    open: AtomicBool,

    track_info_changed: AtomicBool,
    track_info: Mutex<TrackInfo>,
}

impl IcedEditorState {
    /// Create a new state for iced's editor.
    pub fn from_size(size: impl Into<Size>, scale_factor: f32) -> Arc<Self> {
        assert!(scale_factor > 0.0);

        Arc::new(Self {
            size: AtomicCell::new(size.into()),
            zoom_factor: AtomicCell::new(scale_factor),
            open: AtomicBool::new(false),
            fallback_scale_factor: AtomicCell::new(None),
            system_scale_factor: AtomicCell::new(None),
            track_info_changed: AtomicBool::new(false),
            track_info: Mutex::new(TrackInfo::default()),
        })
    }

    pub fn size(&self) -> Size {
        self.size.load()
    }

    fn system_scale_factor(&self) -> f32 {
        let scale_factor = self.system_scale_factor.load();
        let fallback_scale_factor = self.fallback_scale_factor.load();

        scale_factor.unwrap_or_else(|| fallback_scale_factor.unwrap_or(1.0))
    }

    pub fn logical_size(&self) -> LogicalSize<f32> {
        let size = self.size.load();
        let scale_factor = self.system_scale_factor();

        size.to_logical(scale_factor as f64)
    }

    pub fn physical_size(&self) -> PhysicalSize<u32> {
        let size = self.size.load();
        let scale_factor = self.system_scale_factor();

        size.to_physical(scale_factor as f64)
    }

    /// The current user zoom (scale) factor. This is applied on top of the
    /// system's scale factor.
    pub fn user_scale_factor(&self) -> f32 {
        self.zoom_factor.load()
    }

    /// Set the user zoom (scale) factor. This is applied on top of the
    /// system's scale factor.
    pub fn set_user_scale_factor(&self, scale_factor: f32) {
        assert!(scale_factor > 0.0);

        self.zoom_factor.store(scale_factor);
    }

    /// Whether the GUI is currently visible.
    pub fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }

    /// Returns the new track information if it has changed since the previous call.
    pub fn new_track_info(&self) -> Option<TrackInfo> {
        if self.track_info_changed.swap(false, Ordering::Acquire) {
            Some(self.track_info.lock().unwrap().clone())
        } else {
            None
        }
    }
}

pub struct IcedNiceContext {
    pub nice_context: GuiContext,
    shared_window_size: SharedWindowSize,
    editor_state: Arc<IcedEditorState>,
}

impl IcedNiceContext {
    /// Whether the GUI is currently visible.
    pub fn is_open(&self) -> bool {
        self.editor_state.is_open()
    }

    pub fn window_size(&self) -> baseview::WindowSize {
        self.shared_window_size.get()
    }

    pub fn request_window_resize(&self, size: LogicalSize<f32>) {
        self.shared_window_size.request_resize(size);
    }

    /// The current user zoom (scale) factor. This is applied on top of the
    /// system's scale factor.
    pub fn user_scale_factor(&self) -> f32 {
        self.editor_state.user_scale_factor()
    }

    /// Set the user zoom (scale) factor. This is applied on top of the
    /// system's scale factor.
    ///
    /// Note, this must be paired with returning the scale factor in
    /// [`Program::scale_factor`] in order for the window to resize.
    pub fn set_user_scale_factor(&self, scale_factor: f32) {
        self.editor_state.set_user_scale_factor(scale_factor);
    }

    /// Sync the editor state with the current window size and zoom factor.
    pub fn sync_window_size(&self) {
        let size = self.window_size();

        self.editor_state
            .system_scale_factor
            .store(Some(size.scale_factor as f32));

        let old_size = self.editor_state.size.load();
        let new_size = match old_size {
            Size::Logical(_) => Size::Logical(size.logical),
            Size::Physical(_) => Size::Physical(size.physical),
        };
        self.editor_state.size.store(new_size);
    }

    /// Returns the new track information if it has changed since the previous call.
    pub fn new_track_info(&self) -> Option<TrackInfo> {
        self.editor_state.new_track_info()
    }
}

impl Clone for IcedNiceContext {
    fn clone(&self) -> Self {
        Self {
            nice_context: self.nice_context.clone(),
            shared_window_size: self.shared_window_size.clone(),
            editor_state: Arc::clone(&self.editor_state),
        }
    }
}
