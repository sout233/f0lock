//! Traits for working with plugin editors.

use bitflags::bitflags;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::error::Error;
use std::ffi::{c_ulong, c_void};
use std::num::{NonZeroIsize, NonZeroU32};
use std::ptr::NonNull;

use crate::context::gui::GuiContext;
use crate::plugin::TrackInfo;

use self::dpi::{LogicalSize, NativeSize, PhysicalSize, Size};

pub mod dpi;

/// An [`EditorHandle`] and window pair spawned with [`Editor::spawn()`].
pub struct SpawnedEditor<E: EditorHandle> {
    /// A handle to the instance of an open [`Editor`].
    ///
    /// When this handle is dropped, the editor instance is also dropped.
    pub handle: E,

    /// The owned window handle
    ///
    /// This will usually be [`baseview::Window`](). This is type-erased to avoid needing nice-plug-core
    /// to depend on `baseview` until it has a stable version.
    ///
    /// When this is dropped, the window should be automatically closed.
    pub window: E::Window,
}

/// A handler for baseview windows to interact with their host.
///
/// (This is a re-implementation of
/// [`baseview::host::HostCallbacks`](https://docs.rs/baseview/latest/baseview/host/trait.HostCallbacks.html)
/// to avoid directly depending on `baseview` until it is stabilized.)
pub trait HostCallbacks: 'static {
    /// Requests the parent window to be resized to accommodate the child window with
    /// the given new size.
    ///
    /// # Errors
    ///
    /// This can return any type of error, indicating the host either failed or denied
    /// to handle the resize request. If it does, the error is logged and the resize
    /// operation is canceled or reverted.
    fn request_resize(&mut self, new_size: Size, scale_factor: f64) -> Result<(), Box<dyn Error>>;

    /// Notifies the host that the child window has been destroyed for a reason outside
    /// the host’s control.
    ///
    /// This can be because the display connection was lost, because the window handler
    /// crashed, or because the window handler decided to close the window itself.
    ///
    /// The host should close its parent window, as it will not show anything useful
    /// anymore.
    fn destroyed(&mut self);
}

/// A special handler for the Window thread to wake up and call methods on the main thread.
///
/// (This is a re-implementation of
/// [`baseview::host::HostMainThreadCaller`](https://docs.rs/baseview/latest/baseview/host/trait.HostMainThreadCaller.html)
/// to avoid directly depending on `baseview` until it is stabilized.)
///
/// # Platform compatibility notes
///
/// This is only needed on X11, as Windows and macOS windows already run on the main thread.
pub trait HostMainThreadCaller: Send + 'static {
    /// Schedules a callback on the main thread.
    ///
    /// # Platform compatibility notes
    ///
    /// Only X11 needs this. This can be implemented as a no-op on Windows and macOS.
    fn call_main_thread(&mut self);
}

/// Callbacks that baseview windows can use to interact with the host.
pub struct HostMethods {
    /// A handler for baseview windows to interact with their host.
    ///
    /// (This is a re-implementation of
    /// [`baseview::host::HostCallbacks`](https://docs.rs/baseview/latest/baseview/host/trait.HostCallbacks.html)
    /// to avoid directly depending on `baseview` until it is stabilized.)
    pub callbacks: Box<dyn HostCallbacks>,

    /// A special handler for the Window thread to wake up and call methods on the main thread.
    ///
    /// (This is a re-implementation of
    /// [`baseview::host::HostMainThreadCaller`](https://docs.rs/baseview/latest/baseview/host/trait.HostMainThreadCaller.html)
    /// to avoid directly depending on `baseview` until it is stabilized.)
    ///
    /// # Platform compatibility notes
    ///
    /// This is only needed on X11, as Windows and macOS windows already run on the main thread.
    pub main_thread_caller: Box<dyn HostMainThreadCaller>,
}

/// A handle to spawned instance of an [`Editor`].
///
/// The host uses this to resize the editor's window and to dispatch key events.
pub trait EditorHandle: Send + 'static {
    /// The window handle typed used by the editor. This will typically be
    /// [`baseview::Window`](https://docs.rs/baseview/latest/baseview/struct.Window.html).
    type Window;

    /// The error type associated with this editor.
    ///
    /// This will typically include
    /// [`baseview::Error`](https://docs.rs/baseview/latest/baseview/struct.Error.html).
    type Error: Error;

    /// Open the window, and block the current thread until the window is
    /// closed. Used only for standalone targets.
    fn run_until_closed(window: Self::Window) -> Result<(), Self::Error>;

    /// Attach the editor's window to the host's parent window.
    fn set_parent(
        &self,
        parent: ParentWindowHandle,
        window: &Self::Window,
    ) -> Result<(), Self::Error>;

    /// Show the window
    ///
    /// This will never be called on the standalone target.
    fn show(&self, window: &Self::Window) -> Result<(), Self::Error>;

    /// Hide the window
    ///
    /// This will never be called on the standalone target.
    fn hide(&self, window: &Self::Window) -> Result<(), Self::Error>;

    /// Called by the wrapper when the host has resized the plugin's view. The
    /// editor should resize its own window and contents to match these dimensions.
    ///
    /// This is the counterpart to [`size()`][Editor::size()]: after a successful
    /// `set_size`, `size()` should report the new dimensions.
    ///
    /// This will never be called on the standalone target.
    fn set_size(&self, new_size: NativeSize<u32>, window: &Self::Window)
    -> Result<(), Self::Error>;

    /// Called by the host in response to a request from the [`HostMainThreadCaller`].
    fn host_main_thread_callback(&self, window: &Self::Window);

    /// Return the closest supported size.
    ///
    /// This will never be called on the standalone target.
    fn adjust_size(
        &self,
        new_size: NativeSize<u32>,
        window: &Self::Window,
    ) -> Option<NativeSize<u32>> {
        let _ = new_size;
        let _ = window;
        None
    }

    /// Called when the host has a new suggested scale factor to use.
    ///
    /// Right now this is never called on macOS since DPI scaling is built into the
    /// operating system there.
    ///
    /// This will never be called on the standalone target.
    fn set_fallback_scale_factor(
        &self,
        scale_factor: f64,
        window: &Self::Window,
    ) -> Result<(), Self::Error> {
        let _ = scale_factor;
        let _ = window;
        Ok(())
    }

    /// Called when the host delivers a virtual-key event to the plugin's
    /// view. Return `true` if the editor consumed the key (the wrapper
    /// will tell the host to skip its own accelerator handling); return
    /// `false` to let the host process the key normally.
    ///
    /// The wrapper only invokes this for non-character "virtual" keys
    /// ([`VirtualKeyCode::Backspace`], the arrow keys, function keys,
    /// modifier presses, etc.). Plain printable characters arrive through
    /// the plugin window's native keyboard path (on macOS, AppKit
    /// `keyDown:` + NSTextInputContext) and are not routed here; consuming
    /// them through this hook would double-dispatch text input.
    ///
    /// Both key-down and key-up events are delivered; `is_down` is
    /// `true` for press, `false` for release. Plug-ins that consume a
    /// key on press should generally also return `true` for the
    /// matching release so the host doesn't pick up the release as a
    /// separate accelerator.
    ///
    /// This is primarily for text-input routing in hosts (notably
    /// REAPER) that intercept certain keys (Space, Backspace, arrows,
    /// Cmd-shortcuts) before they reach the plugin's native view. The
    /// editor should only return `true` if a text input in the editor
    /// currently has focus and can consume the key.
    ///
    /// This will never be called on the standalone target.
    ///
    /// # Parameters
    ///
    /// - `key_code`: the virtual key the host reports.
    /// - `is_down`: `true` for key-down, `false` for key-up.
    /// - `modifiers`: which modifier keys were held when the event was
    ///   generated.
    fn on_virtual_key_from_host(
        &self,
        key_code: VirtualKeyCode,
        is_down: bool,
        modifiers: Modifiers,
    ) -> bool {
        let _ = key_code;
        let _ = is_down;
        let _ = modifiers;
        false
    }

    /// Called when the plugin's state has changed (i.e. a preset was loaded). The
    /// editor should rescan all of its parameters.
    ///
    /// Generally you will want to trigger a redraw when this is called.
    fn state_changed(&self) {}

    /// Called whenever a specific parameter's value has changed. You don't
    /// need to do anything with this, but this can be used to force a redraw when the host sends a
    /// new value for a parameter or when a parameter change sent to the host gets processed.
    ///
    /// Generally you will want to trigger a redraw when this is called.
    fn param_value_changed(&self, id: &str, normalized_value: f32);

    /// Called whenever a specific parameter's monophonic modulation value has changed.
    ///
    /// Generally you will want to trigger a redraw when this is called.
    fn param_modulation_changed(&self, id: &str, modulation_offset: f32);
}

/// An editor for a [`Plugin`][crate::plugin::Plugin].
pub trait Editor: Send {
    type Handle: EditorHandle;

    /// Create an instance of the plugin's editor and embed it in the parent window. As explained in
    /// [`Plugin::editor()`][crate::plugin::Plugin::editor()], you can then read the parameter
    /// values directly from your [`Params`][crate::params::Params] object, and modifying the
    /// values can be done using the functions on the [`ParamSetter`][crate::context::gui::ParamSetter].
    /// When you change a parameter value that way it will be broadcasted to the host and also
    /// updated in your [`Params`][crate::params::Params] struct.
    ///
    /// This function should return a handle to the editor, which will be dropped when the editor
    /// gets closed. Implement the [`Drop`] trait on the returned handle if you need to explicitly
    /// handle the editor's closing behavior.
    ///
    /// If an error is returned, then the editor will not open.
    ///
    /// If [`EditorHandle::set_fallback_scale_factor()`] has been called, then any created
    /// windows should have their sizes multiplied by that factor.
    ///
    /// The wrapper guarantees that a previous handle has been dropped before this function is
    /// called again.
    //
    // TODO: Think of how this would work with the event loop. On Linux the wrapper must provide a
    //       timer using VST3's `IRunLoop` interface, but on Window and macOS the window would
    //       normally register its own timer. Right now we just ignore this because it would
    //       otherwise be basically impossible to have this still be GUI-framework agnostic. Any
    //       callback that deos involve actual GUI operations will still be spooled to the IRunLoop
    //       instance.
    fn spawn(
        &self,
        parent: Option<ParentWindowHandle>,
        wait_for_parent: bool,
        fallback_scale_factor: Option<f64>,
        gui_context: GuiContext,
        host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>>;

    /// Returns the (current) size of the editor.
    ///
    /// This size is represented in the platform's native pixels (physical pixels on Windows and Linux,
    /// and in logical pixels on macOS.)
    fn size(&self) -> NativeSize<u32>;

    /// Describes whether and how the host may resize this editor. The wrapper
    /// reads this to answer the host's resize-capability queries (CLAP's
    /// `gui.can_resize` / `gui.get_resize_hints`, VST3's `canResize`).
    ///
    /// The default is [`ResizeHint::default()`], which is **not** resizable, so
    /// editors keep their fixed-size behavior unless they opt in. An editor that
    /// supports host resizing should return a hint with `can_resize: true` (and
    /// usually also implement [`EditorHandle::set_size()`] to apply the new
    /// size). See [`ResizeHint`] for the per-axis and aspect-ratio options.
    fn resize_hint(&self) -> ResizeHint {
        ResizeHint::default()
    }

    /// Called when the provided track information has changed.
    ///
    /// Generally you will want to trigger a redraw when this is called, if your GUI uses the
    /// track informatioin.
    fn track_info_updated(&self, info: TrackInfo) {
        let _ = info;
    }

    /// Called right after the editor is initialized when running in standalone mode (if the
    /// user has enabled the `unsafe_standalone_dpi_fix` feature in nice-plug).
    ///
    /// Here you can call
    /// [`baseview::assume_standalone_in_process()`](https://docs.rs/baseview/0.3.4/baseview/fn.assume_standalone_in_process.html)
    /// as a fix for <https://github.com/RustAudio/baseview/issues/321>.
    ///
    /// # Safety
    ///
    /// This method must *NOT* be called in the following cases:
    /// * The current binary is a plugin that can be loaded into an external host
    /// * Multiple baseview versions are present in the final binary
    /// * baseview is being used in conjunction with other platform windowing libraries (e.g. winit, SDL, etc.)
    /// * The current process may host other plugins that need to interact with the platform's GUI capabilities.
    unsafe fn assume_standalone_in_process(&self) {}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DummyEditorError;
impl Error for DummyEditorError {}
impl std::fmt::Display for DummyEditorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Plugin does not implement an editor")
    }
}

impl EditorHandle for () {
    type Window = ();
    type Error = DummyEditorError;

    fn run_until_closed(_window: Self::Window) -> Result<(), Self::Error> {
        Err(DummyEditorError)
    }

    fn set_parent(
        &self,
        _parent: ParentWindowHandle,
        _window: &Self::Window,
    ) -> Result<(), Self::Error> {
        Err(DummyEditorError)
    }

    fn show(&self, _window: &Self::Window) -> Result<(), Self::Error> {
        Err(DummyEditorError)
    }

    fn hide(&self, _window: &Self::Window) -> Result<(), Self::Error> {
        Err(DummyEditorError)
    }

    fn host_main_thread_callback(&self, _window: &Self::Window) {}

    fn set_size(
        &self,
        _new_size: NativeSize<u32>,
        _window: &Self::Window,
    ) -> Result<(), Self::Error> {
        Err(DummyEditorError)
    }

    fn param_value_changed(&self, _id: &str, _normalized_value: f32) {}

    fn param_modulation_changed(&self, _id: &str, _modulation_offset: f32) {}
}

impl Editor for () {
    type Handle = ();

    fn spawn(
        &self,
        _parent: Option<ParentWindowHandle>,
        _wait_for_parent: bool,
        _fallback_scale_factor: Option<f64>,
        _gui_context: GuiContext,
        _host: Option<HostMethods>,
    ) -> Result<SpawnedEditor<Self::Handle>, Box<dyn Error>> {
        Err(String::from("Plugin does not implement an editor").into())
    }

    fn size(&self) -> NativeSize<u32> {
        NativeSize {
            width: 0,
            height: 0,
        }
    }
}

/// The minimum/maximum size constraints for an editor window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeConstraints {
    /// Min/max size in logical points (before scaling is applied).
    Logical {
        min_size: Option<LogicalSize<f32>>,
        max_size: Option<LogicalSize<f32>>,
    },
    /// Min/max size in physical pixels.
    Physical {
        min_size: Option<PhysicalSize<u32>>,
        max_size: Option<PhysicalSize<u32>>,
    },
}

impl SizeConstraints {
    /// Minimum size in logical points (before scaling is applied).
    pub const fn min_logical_size(min_size: LogicalSize<f32>) -> Self {
        Self::Logical {
            min_size: Some(min_size),
            max_size: None,
        }
    }

    /// Minimum size in physical pixels.
    pub const fn min_physical_size(min_size: PhysicalSize<u32>) -> Self {
        Self::Physical {
            min_size: Some(min_size),
            max_size: None,
        }
    }

    /// Min/max size in logical points (before scaling is applied).
    pub const fn logical(
        min_size: Option<LogicalSize<f32>>,
        max_size: Option<LogicalSize<f32>>,
    ) -> Self {
        Self::Logical { min_size, max_size }
    }

    /// Min/max size in physical pixels.
    pub const fn physical(
        min_size: Option<PhysicalSize<u32>>,
        max_size: Option<PhysicalSize<u32>>,
    ) -> Self {
        Self::Physical { min_size, max_size }
    }
}

impl Default for SizeConstraints {
    fn default() -> Self {
        Self::Logical {
            min_size: None,
            max_size: None,
        }
    }
}

/// Describes whether and how a host may resize an [`Editor`], returned from
/// [`Editor::resize_hint()`].
///
/// The default is non-resizable (`can_resize: false`), matching the previous
/// fixed-size behavior. To make an editor resizable, return a hint with
/// `can_resize: true`; the per-axis flags and aspect-ratio fields refine how.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResizeHint {
    /// Whether the host may resize the editor at all. Drives CLAP's
    /// `gui.can_resize` and VST3's `canResize`. When `false`, the other fields
    /// are ignored.
    pub can_resize: bool,
    /// Whether the width may change. Only meaningful when `can_resize` is `true`.
    pub can_resize_horizontally: bool,
    /// Whether the height may change. Only meaningful when `can_resize` is `true`.
    pub can_resize_vertically: bool,
    /// If `true`, the host should keep the editor's aspect ratio fixed at
    /// `aspect_ratio_width : aspect_ratio_height` while resizing.
    pub preserve_aspect_ratio: bool,
    /// Aspect-ratio numerator (only used when `preserve_aspect_ratio` is `true`).
    pub aspect_ratio_width: u32,
    /// Aspect-ratio denominator (only used when `preserve_aspect_ratio` is `true`).
    pub aspect_ratio_height: u32,
    /// The minimum/maximum size constraints for the window.
    pub size_constraints: SizeConstraints,
}

impl Default for ResizeHint {
    fn default() -> Self {
        // Not resizable by default, so editors keep their fixed-size behavior
        // unless they explicitly opt in.
        Self::NON_RESIZABLE
    }
}

impl ResizeHint {
    /// A non-resizable editor. This is the default value.
    pub const NON_RESIZABLE: Self = Self {
        size_constraints: SizeConstraints::Logical {
            min_size: None,
            max_size: None,
        },
        can_resize: false,
        can_resize_horizontally: false,
        can_resize_vertically: false,
        preserve_aspect_ratio: false,
        aspect_ratio_width: 1,
        aspect_ratio_height: 1,
    };

    /// A freely resizable editor: both axes, no aspect-ratio lock. Convenience
    /// for the common case.
    pub const RESIZABLE: Self = Self {
        can_resize: true,
        can_resize_horizontally: true,
        can_resize_vertically: true,
        ..Self::NON_RESIZABLE
    };

    /// A non-resizable editor. This is the default value.
    pub const fn non_resizable() -> Self {
        Self::NON_RESIZABLE
    }

    /// A freely resizable editor: both axes, no aspect-ratio lock. Convenience
    /// for the common case.
    pub const fn resizable() -> Self {
        Self::RESIZABLE
    }

    /// Set the minimum size for the window in logical points (before scaling is applied).
    pub const fn with_min_logical_size(mut self, min_size: LogicalSize<f32>) -> Self {
        self.size_constraints = SizeConstraints::Logical {
            min_size: Some(min_size),
            max_size: None,
        };
        self
    }

    /// Set the minimum/maximum size for the window in logical points (before scaling is applied).
    pub const fn with_min_max_logical_size(
        mut self,
        min_size: Option<LogicalSize<f32>>,
        max_size: Option<LogicalSize<f32>>,
    ) -> Self {
        self.size_constraints = SizeConstraints::Logical { min_size, max_size };
        self
    }

    /// Set the minimum size for the window in physical pixels.
    pub const fn with_min_physical_size(mut self, min_size: PhysicalSize<u32>) -> Self {
        self.size_constraints = SizeConstraints::Physical {
            min_size: Some(min_size),
            max_size: None,
        };
        self
    }

    /// Set the minimum/maximum size for the window in physical pixels.
    pub const fn with_min_max_physical_size(
        mut self,
        min_size: Option<PhysicalSize<u32>>,
        max_size: Option<PhysicalSize<u32>>,
    ) -> Self {
        self.size_constraints = SizeConstraints::Physical { min_size, max_size };
        self
    }

    /// Use the given minimum/maximum size constraints.
    pub const fn with_size_constraints(mut self, size_constraints: SizeConstraints) -> Self {
        self.size_constraints = size_constraints;
        self
    }

    /// Lock the window to the given aspect ratio.
    ///
    /// * `aspect_ratio_width`: aspect-ratio numerator
    /// * `aspect_ratio_height`: aspect-ratio denominator
    pub const fn with_aspect_ratio(
        mut self,
        aspect_ratio_width: u32,
        aspect_ratio_height: u32,
    ) -> Self {
        assert!(aspect_ratio_width != 0);
        assert!(aspect_ratio_height != 0);

        self.aspect_ratio_width = aspect_ratio_width;
        self.aspect_ratio_height = aspect_ratio_height;

        self
    }

    /// Returns whether or not the given size in physical pixels is valid.
    pub fn is_size_valid(
        &self,
        new_size: NativeSize<u32>,
        current_size: NativeSize<u32>,
        scale_factor: f64,
    ) -> bool {
        let adjusted_size = self.adjust_size(new_size, current_size, scale_factor);
        new_size == adjusted_size
    }

    /// Adjust the new requested size to the closest size that is compatible
    /// with this plugin.
    pub fn adjust_size(
        &self,
        new_size: NativeSize<u32>,
        current_size: NativeSize<u32>,
        scale_factor: f64,
    ) -> NativeSize<u32> {
        if !self.can_resize {
            return current_size;
        }

        let mut new_physical_size = new_size.to_physical(scale_factor);
        let current_physical_size = current_size.to_physical(scale_factor);

        let (min_phy_size, max_phy_size) = match self.size_constraints {
            SizeConstraints::Logical { min_size, max_size } => (
                min_size.map(|s| PhysicalSize {
                    width: (s.width as f64 * scale_factor).round() as u32,
                    height: (s.height as f64 * scale_factor).round() as u32,
                }),
                max_size.map(|s| PhysicalSize {
                    width: (s.width as f64 * scale_factor).round() as u32,
                    height: (s.height as f64 * scale_factor).round() as u32,
                }),
            ),
            SizeConstraints::Physical { min_size, max_size } => (min_size, max_size),
        };

        if let Some(min_size) = min_phy_size {
            new_physical_size.width = new_physical_size.width.max(min_size.width);
            new_physical_size.height = new_physical_size.height.max(min_size.height);
        }
        if let Some(max_size) = max_phy_size {
            new_physical_size.width = new_physical_size.width.min(max_size.width);
            new_physical_size.height = new_physical_size.height.min(max_size.height);
        }

        new_physical_size.width = new_physical_size.width.max(1);
        new_physical_size.height = new_physical_size.height.max(1);

        if self.preserve_aspect_ratio {
            let adjusted_width = (new_physical_size.height as f32 * self.aspect_ratio_width as f32
                / self.aspect_ratio_height as f32)
                .round() as u32;

            if let Some(min_size) = min_phy_size
                && adjusted_width < min_size.width
            {
                new_physical_size = min_size;
            } else if let Some(max_size) = max_phy_size
                && adjusted_width > max_size.width
            {
                new_physical_size = max_size;
            } else {
                new_physical_size.width = adjusted_width;
            }
        } else {
            if !self.can_resize_horizontally {
                new_physical_size.width = current_physical_size.width;
            }
            if !self.can_resize_vertically {
                new_physical_size.height = current_physical_size.height;
            }
        }

        NativeSize::from_size(new_physical_size.into(), scale_factor)
    }
}

/// A raw window handle for platform and GUI framework agnostic editors. This implements
/// [`HasWindowHandle`] so it can be used directly with GUI libraries that use the same
/// [`raw_window_handle`] version. If the library links against a different version of
/// `raw_window_handle`, then you'll need to wrap around this type and implement the trait yourself.
#[derive(Debug, Clone, Copy)]
pub enum ParentWindowHandle {
    /// The ID of the host's parent window. Used with X11.
    XlibWindow(c_ulong),
    /// The ID of the host's parent window. Used with X11.
    XcbWindow(NonZeroU32),
    /// A handle to the host's parent window. Used only on macOS.
    AppKitNsView(NonNull<c_void>),
    /// A handle to the host's parent window. Used only on Windows.
    Win32Hwnd(NonZeroIsize),
}

impl HasWindowHandle for ParentWindowHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        let raw = match *self {
            ParentWindowHandle::XlibWindow(window) => {
                RawWindowHandle::Xlib(raw_window_handle::XlibWindowHandle::new(window))
            }
            ParentWindowHandle::XcbWindow(window) => {
                RawWindowHandle::Xcb(raw_window_handle::XcbWindowHandle::new(window))
            }
            ParentWindowHandle::AppKitNsView(ns_view) => {
                RawWindowHandle::AppKit(raw_window_handle::AppKitWindowHandle::new(ns_view))
            }
            ParentWindowHandle::Win32Hwnd(hwnd) => {
                RawWindowHandle::Win32(raw_window_handle::Win32WindowHandle::new(hwnd))
            }
        };

        Ok(unsafe { raw_window_handle::WindowHandle::borrow_raw(raw) })
    }
}

/// A non-character key delivered to
/// [`EditorHandle::on_virtual_key_from_host`]. Variant names mirror standard
/// keyboard nomenclature; printable ASCII characters never appear here
/// because they flow through the plugin window's native keyboard path
/// instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VirtualKeyCode {
    Backspace,
    Tab,
    Clear,
    Return,
    Pause,
    Escape,
    Space,
    Next,
    End,
    Home,
    ArrowLeft,
    ArrowUp,
    ArrowRight,
    ArrowDown,
    PageUp,
    PageDown,
    Select,
    Print,
    /// Numpad enter (distinct from [`VirtualKeyCode::Return`]).
    NumpadEnter,
    Snapshot,
    Insert,
    Delete,
    Help,
    Numpad0,
    Numpad1,
    Numpad2,
    Numpad3,
    Numpad4,
    Numpad5,
    Numpad6,
    Numpad7,
    Numpad8,
    Numpad9,
    NumpadMultiply,
    NumpadAdd,
    NumpadSeparator,
    NumpadSubtract,
    NumpadDecimal,
    NumpadDivide,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    NumLock,
    ScrollLock,
    /// Shift key, delivered as a press/release on the modifier itself.
    /// For most text-input purposes you want
    /// [`Modifiers::SHIFT`] on the event's modifier set instead; the
    /// dedicated press is useful only for editors that react to
    /// modifier-only gestures.
    Shift,
    /// Control key (macOS Ctrl, platform-Ctrl elsewhere). See the note
    /// on [`VirtualKeyCode::Shift`].
    Control,
    /// Alt / Option key. See the note on [`VirtualKeyCode::Shift`].
    Alt,
    Equals,
    ContextMenu,
    MediaPlay,
    MediaStop,
    MediaPrevTrack,
    MediaNextTrack,
    VolumeUp,
    VolumeDown,
    F13,
    F14,
    F15,
    F16,
    F17,
    F18,
    F19,
    F20,
    F21,
    F22,
    F23,
    F24,
    /// Super / Command / Windows key. See the note on
    /// [`VirtualKeyCode::Shift`].
    Super,
}

bitflags! {
    /// Modifier keys held while a keyboard event was generated, as
    /// reported by [`Editor::on_virtual_key_from_host`]. Use the
    /// standard `bitflags` API (`contains`, `intersects`, `is_empty`,
    /// etc.) to query individual modifiers.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
    pub struct Modifiers: u8 {
        /// Shift key.
        const SHIFT = 1 << 0;
        /// Alt / Option key.
        const ALT = 1 << 1;
        /// Command key. On Windows / Linux this is typically the Ctrl
        /// key. See [`Modifiers::CONTROL`] for the macOS Control key
        /// specifically.
        const COMMAND = 1 << 2;
        /// Control key (macOS Ctrl, distinct from
        /// [`Modifiers::COMMAND`]).
        const CONTROL = 1 << 3;
    }
}
