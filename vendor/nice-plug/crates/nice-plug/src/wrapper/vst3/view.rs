use crossbeam::atomic::AtomicCell;
use fragile::Fragile;
use nice_plug_core::editor::HostMainThreadCaller;
use nice_plug_core::editor::dpi::{NativeSize, Size};
use nice_plug_core::editor::{
    Editor, EditorHandle, HostCallbacks, HostMethods, ParentWindowHandle,
};
use parking_lot::{Mutex, RwLock};
use std::error::Error;
use std::ffi::{CStr, c_ulong, c_void};
use std::num::NonZeroIsize;
use std::ptr::NonNull;
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Weak};
use vst3::Steinberg::{
    FIDString, TBool, char16, int16, kInvalidArgument, kNotImplemented, kPlatformTypeHWND,
    kPlatformTypeNSView, kPlatformTypeX11EmbedWindowID, kResultFalse, kResultOk, tresult,
};
use vst3::Steinberg::{
    IPlugFrame, IPlugView, IPlugViewContentScaleSupport,
    IPlugViewContentScaleSupport_::ScaleFactor, IPlugViewContentScaleSupportTrait, IPlugViewTrait,
    ViewRect,
};
use vst3::{Class, ComPtr, ComRef, ComWrapper};

use super::inner::{Task, WrapperInner};
use crate::editor::{Modifiers, VirtualKeyCode};
use crate::wrapper::vst3::{Vst3Plugin, util::fid_matches};

/// Lowest VST3 virtual key code (`KEY_BACK` in the VST3 SDK
/// `VirtualKeyCodes` enum, `pluginterfaces/base/keycodes.h`). Values
/// below this are not virtual keys in the VST3 sense.
const VKEY_FIRST_CODE: i16 = 1;

/// Highest VST3 virtual key code (`KEY_SUPER` in the VST3 SDK
/// `VirtualKeyCodes` enum). Values at `VKEY_FIRST_ASCII = 128` and
/// above encode printable ASCII characters rather than virtual keys
/// and are not routed to [`Editor::on_virtual_key_from_host`].
const VKEY_LAST_CODE: i16 = 77;

/// Maps a VST3 virtual key code (as passed to `IPlugView::onKeyDown`
/// / `onKeyUp`) to the corresponding [`VirtualKeyCode`] variant.
/// Callers should gate on `VKEY_FIRST_CODE..=VKEY_LAST_CODE` first;
/// codes outside that range (including `0`, and ASCII offsets at
/// `VKEY_FIRST_ASCII = 128` and above) return `None`.
fn vst3_virtual_key_code(raw: i16) -> Option<VirtualKeyCode> {
    Some(match raw {
        1 => VirtualKeyCode::Backspace,
        2 => VirtualKeyCode::Tab,
        3 => VirtualKeyCode::Clear,
        4 => VirtualKeyCode::Return,
        5 => VirtualKeyCode::Pause,
        6 => VirtualKeyCode::Escape,
        7 => VirtualKeyCode::Space,
        8 => VirtualKeyCode::Next,
        9 => VirtualKeyCode::End,
        10 => VirtualKeyCode::Home,
        11 => VirtualKeyCode::ArrowLeft,
        12 => VirtualKeyCode::ArrowUp,
        13 => VirtualKeyCode::ArrowRight,
        14 => VirtualKeyCode::ArrowDown,
        15 => VirtualKeyCode::PageUp,
        16 => VirtualKeyCode::PageDown,
        17 => VirtualKeyCode::Select,
        18 => VirtualKeyCode::Print,
        19 => VirtualKeyCode::NumpadEnter,
        20 => VirtualKeyCode::Snapshot,
        21 => VirtualKeyCode::Insert,
        22 => VirtualKeyCode::Delete,
        23 => VirtualKeyCode::Help,
        24 => VirtualKeyCode::Numpad0,
        25 => VirtualKeyCode::Numpad1,
        26 => VirtualKeyCode::Numpad2,
        27 => VirtualKeyCode::Numpad3,
        28 => VirtualKeyCode::Numpad4,
        29 => VirtualKeyCode::Numpad5,
        30 => VirtualKeyCode::Numpad6,
        31 => VirtualKeyCode::Numpad7,
        32 => VirtualKeyCode::Numpad8,
        33 => VirtualKeyCode::Numpad9,
        34 => VirtualKeyCode::NumpadMultiply,
        35 => VirtualKeyCode::NumpadAdd,
        36 => VirtualKeyCode::NumpadSeparator,
        37 => VirtualKeyCode::NumpadSubtract,
        38 => VirtualKeyCode::NumpadDecimal,
        39 => VirtualKeyCode::NumpadDivide,
        40 => VirtualKeyCode::F1,
        41 => VirtualKeyCode::F2,
        42 => VirtualKeyCode::F3,
        43 => VirtualKeyCode::F4,
        44 => VirtualKeyCode::F5,
        45 => VirtualKeyCode::F6,
        46 => VirtualKeyCode::F7,
        47 => VirtualKeyCode::F8,
        48 => VirtualKeyCode::F9,
        49 => VirtualKeyCode::F10,
        50 => VirtualKeyCode::F11,
        51 => VirtualKeyCode::F12,
        52 => VirtualKeyCode::NumLock,
        53 => VirtualKeyCode::ScrollLock,
        54 => VirtualKeyCode::Shift,
        55 => VirtualKeyCode::Control,
        56 => VirtualKeyCode::Alt,
        57 => VirtualKeyCode::Equals,
        58 => VirtualKeyCode::ContextMenu,
        59 => VirtualKeyCode::MediaPlay,
        60 => VirtualKeyCode::MediaStop,
        61 => VirtualKeyCode::MediaPrevTrack,
        62 => VirtualKeyCode::MediaNextTrack,
        63 => VirtualKeyCode::VolumeUp,
        64 => VirtualKeyCode::VolumeDown,
        65 => VirtualKeyCode::F13,
        66 => VirtualKeyCode::F14,
        67 => VirtualKeyCode::F15,
        68 => VirtualKeyCode::F16,
        69 => VirtualKeyCode::F17,
        70 => VirtualKeyCode::F18,
        71 => VirtualKeyCode::F19,
        72 => VirtualKeyCode::F20,
        73 => VirtualKeyCode::F21,
        74 => VirtualKeyCode::F22,
        75 => VirtualKeyCode::F23,
        76 => VirtualKeyCode::F24,
        77 => VirtualKeyCode::Super,
        _ => return None,
    })
}

/// Convert the raw VST3 modifier bitmask (`KeyModifier` in the VST3
/// SDK: `kShiftKey = 1`, `kAlternateKey = 2`, `kCommandKey = 4`,
/// `kControlKey = 8`) to a [`Modifiers`] bitflags value. The VST3
/// spec only defines bits 0-3; higher bits from a misbehaving host
/// are dropped.
fn vst3_modifiers(raw: i16) -> Modifiers {
    Modifiers::from_bits_truncate(((raw as u32) & 0x0F) as u8)
}

// Thanks for putting this behind a platform-specific ifdef...
// NOTE: This should also be used on the BSDs, which should now be possible since vst3-sys is replaced by the vst3 crate. Because the vst3 crate also exposes these interfaces to other operating systems.
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
use {
    crate::event_loop::{EventLoop, MainThreadExecutor, TASK_QUEUE_CAPACITY},
    crossbeam::queue::ArrayQueue,
    libc,
    std::cell::Cell,
    vst3::Steinberg::Linux::{
        FileDescriptor, IEventHandler, IEventHandlerTrait, IRunLoop, IRunLoopTrait,
    },
};

/// The plugin's [`IPlugView`] instance created in [`IEditController::create_view()`] if `P` has an
/// editor. This is managed separately so the lifetime bounds match up.
pub(crate) struct WrapperView<P: Vst3Plugin> {
    inner: Weak<WrapperInner<P>>,
    editor: Weak<Mutex<P::Editor>>,

    /// The `IPlugFrame` instance passed by the host during [IPlugView::set_frame()].
    plug_frame: RwLock<Option<ComPtr<IPlugFrame>>>,
    /// Allows handling events events on the host's GUI thread when using Linux. Needed because
    /// otherwise REAPER doesn't like us very much.
    #[cfg(all(target_family = "unix", not(target_os = "macos")))]
    run_loop_event_handler: RwLock<Option<ComWrapper<RunLoopEventHandler<P>>>>,

    /// The DPI scaling factor as passed to the [IPlugViewContentScaleSupport::set_scale_factor()]
    /// function. Defaults to 1.0, and will be kept there on macOS. When reporting and handling size
    /// the sizes communicated to and from the DAW should be scaled by this factor since nice-plug's
    /// APIs only deal in logical pixels.
    fallback_scale_factor: AtomicCell<Option<f32>>,
}

impl<P: Vst3Plugin> Class for WrapperView<P> {
    type Interfaces = (IPlugView, IPlugViewContentScaleSupport);
}

/// Allow handling tasks on the host's GUI thread on Linux. This doesn't need to be a separate
/// struct, but vst3-sys did not let us implement interfaces conditionally and the interface is
/// only exposed when compiling on Linux. This might be fixable since we use the vst3 crate.
/// The struct will register itself when calling [`RunLoopEventHandler::new()`] and it will
/// unregister itself when it gets dropped.
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
struct RunLoopEventHandler<P: Vst3Plugin> {
    /// We need access to the inner wrapper so we that we can post any outstanding tasks there when
    /// this object gets dropped so no work is lost.
    inner: Weak<WrapperInner<P>>,

    /// The host's run loop interface. This lets us run tasks on the same thread as the host's UI.
    run_loop: ComPtr<IRunLoop>,

    /// We need a Unix domain socket the host can poll to know that we have an event to handle. In
    /// theory eventfd would be much better suited for this, but Ardour doesn't respond to fds that
    /// aren't sockets. So instead, we will write a single byte here for every message we should
    /// handle.
    socket_read_fd: i32,
    socket_write_fd: i32,

    /// A queue of tasks that still need to be performed. Because CLAP lets the plugin request a
    /// host callback directly, we don't need to use the OsEventLoop we use in our other plugin
    /// implementations. Instead, we'll post tasks to this queue, ask the host to call
    /// [`on_main_thread()`][Self::on_main_thread()] on the main thread, and then continue to pop
    /// tasks off this queue there until it is empty.
    tasks: ArrayQueue<Task<P>>,

    /// A self-referencing pointer to the outer `ComWrapper<RunLoopEventHandler>`, needed to call
    /// `IRunLoop::unregisterEventHandler()` when this object gets dropped.
    event_handler_ptr: EventHandlerSelfRefPtr,

    notified: AtomicBool,
}

/// A self-referencing pointer to the outer `ComWrapper<RunLoopEventHandler>`, needed to call
/// `IRunLoop::unregisterEventHandler()` when this object gets dropped.
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
struct EventHandlerSelfRefPtr(Cell<*mut IEventHandler>);

// Safety: `ComWrapper<RunLoopEventHandler>` is Send + Sync, so the raw self-referential
// pointer is also Send + Sync.
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
unsafe impl Send for EventHandlerSelfRefPtr {}
#[cfg(all(target_family = "unix", not(target_os = "macos")))]
unsafe impl Sync for EventHandlerSelfRefPtr {}

#[cfg(all(target_family = "unix", not(target_os = "macos")))]
impl<P: Vst3Plugin> Class for RunLoopEventHandler<P> {
    type Interfaces = (IEventHandler,);
}

impl<P: Vst3Plugin> WrapperView<P> {
    pub fn new(inner: Weak<WrapperInner<P>>, editor: Weak<Mutex<P::Editor>>) -> Self {
        Self {
            inner,
            editor,
            plug_frame: RwLock::new(None),
            #[cfg(all(target_family = "unix", not(target_os = "macos")))]
            run_loop_event_handler: RwLock::new(None),
            fallback_scale_factor: AtomicCell::new(None),
        }
    }

    /// Ask the host to resize the view to the size specified by [`Editor::size()`]. Will return false
    /// if the host doesn't like you. This **needs** to be run from the GUI thread.
    ///
    /// # Safety
    ///
    /// May cause memory corruption in Linux REAPER when called from outside of the `IRunLoop`.
    #[must_use]
    pub unsafe fn request_resize(
        this: &ComWrapper<Self>,
        new_size: Size,
        scale_factor: f64,
    ) -> bool {
        use vst3::Steinberg::IPlugFrameTrait;

        let Some(inner) = Weak::upgrade(&(**this).inner) else {
            return false;
        };

        if !inner.is_editor_open.load(Ordering::SeqCst) {
            return false;
        }

        match &*this.plug_frame.read() {
            Some(plug_frame) => {
                let native_size: NativeSize<u32> = NativeSize::from_size(new_size, scale_factor);

                let mut size = ViewRect {
                    left: 0,
                    top: 0,
                    right: native_size.width as i32,
                    bottom: native_size.height as i32,
                };

                let plug_view = this.as_com_ref::<IPlugView>().unwrap();
                let result = unsafe { plug_frame.resizeView(plug_view.as_ptr(), &mut size) };

                #[cfg(debug_assertions)]
                if result != kResultOk {
                    crate::nice_warn!(
                        "The host denied to resize the window to {:?}, we currently don't handle \
                         this for VST3 plugins",
                        new_size
                    );
                }

                result == kResultOk
            }
            None => false,
        }
    }

    /// If the host supports `IRunLoop`, then this will post the task to a task queue that will be
    /// run on the host's UI thread. If not, then this will return an `Err` value containing the
    /// task so it can be run elsewhere.
    #[cfg(all(target_family = "unix", not(target_os = "macos")))]
    pub fn do_maybe_in_run_loop(&self, task: Task<P>) -> Result<(), Task<P>> {
        match &*self.run_loop_event_handler.read() {
            Some(run_loop) => run_loop.post_task(task),
            None => Err(task),
        }
    }

    /// If the host supports `IRunLoop`, then this will post the task to a task queue that will be
    /// run on the host's UI thread. If not, then this will return an `Err` value containing the
    /// task so it can be run elsewhere.
    #[cfg(not(target_os = "linux"))]
    pub fn do_maybe_in_run_loop(&self, task: Task<P>) -> Result<(), Task<P>> {
        Err(task)
    }

    /// Forward a VST3 `IPlugView::onKey{Down,Up}` event to the editor's
    /// host-virtual-key hook.
    ///
    /// Only forwards keys that carry a VST3 virtual key code in
    /// [`VKEY_FIRST_CODE`]`..=`[`VKEY_LAST_CODE`]. Those are the keys
    /// the host (notably REAPER) may intercept as accelerators before
    /// they reach our native view: Space, Backspace, arrows, function
    /// keys, modifier-only presses, etc. ASCII characters arrive with
    /// `key_code >= VKEY_FIRST_ASCII (128)` per the VST3 SDK
    /// (`pluginterfaces/base/keycodes.h`); those flow through the
    /// plug-in window's native keyboard path (on macOS, AppKit
    /// `keyDown:` + NSTextInputContext) and consuming them here would
    /// double-dispatch text input.
    fn dispatch_virtual_key(&self, key_code: i16, is_down: bool, modifiers: i16) -> tresult {
        if !(VKEY_FIRST_CODE..=VKEY_LAST_CODE).contains(&key_code) {
            return kResultFalse;
        }
        let Some(key_code) = vst3_virtual_key_code(key_code) else {
            return kResultFalse;
        };
        let modifiers = vst3_modifiers(modifiers);

        if let Some(inner) = self.inner.upgrade()
            && let Some(editor_window) = inner.editor_window.borrow().as_ref()
        {
            if editor_window
                .get()
                .handle
                .on_virtual_key_from_host(key_code, is_down, modifiers)
            {
                kResultOk
            } else {
                kResultFalse
            }
        } else {
            kResultFalse
        }
    }
}

#[cfg(all(target_family = "unix", not(target_os = "macos")))]
impl<P: Vst3Plugin> RunLoopEventHandler<P> {
    pub fn new(inner: Weak<WrapperInner<P>>, run_loop: ComPtr<IRunLoop>) -> ComWrapper<Self> {
        let mut sockets = [0i32; 2];
        assert_eq!(
            unsafe {
                libc::socketpair(
                    libc::AF_UNIX,
                    libc::SOCK_STREAM | libc::SOCK_CLOEXEC | libc::SOCK_NONBLOCK,
                    0,
                    sockets.as_mut_ptr(),
                )
            },
            0
        );
        let [socket_read_fd, socket_write_fd] = sockets;

        let handler = ComWrapper::new(RunLoopEventHandler {
            inner,
            run_loop,
            socket_read_fd,
            socket_write_fd,
            tasks: ArrayQueue::new(TASK_QUEUE_CAPACITY),
            event_handler_ptr: EventHandlerSelfRefPtr(Cell::new(std::ptr::null_mut())),
            notified: AtomicBool::new(false),
        });
        let event_handler_ptr = handler.to_com_ptr::<IEventHandler>().unwrap().into_raw();

        // Safety: `event_handler_ptr` is a self-referential pointer to the outer
        // `ComWrapper<RunLoopEventHandler>`, so it is valid for the lifetime of
        // this struct.
        handler.event_handler_ptr.0.set(event_handler_ptr);

        assert_eq!(
            unsafe {
                handler
                    .run_loop
                    .registerEventHandler(event_handler_ptr, handler.socket_read_fd)
            },
            kResultOk
        );

        handler
    }

    /// Post a task to the tasks queue so it will be run on the host's GUI thread later. Returns the
    /// task if the queue is full and the task could not be posted.
    pub fn post_task(&self, task: Task<P>) -> Result<(), Task<P>> {
        self.tasks.push(task)?;

        if !self.notified.swap(true, Ordering::SeqCst) {
            // We need to use a Unix domain socket to let the host know to call our event handler. In
            // theory eventfd would be more suitable here, but Ardour does not support that. This is
            // read again in `Self::on_fd_is_set()`.
            let notify_value = 1i8;
            const NOTIFY_VALUE_SIZE: usize = std::mem::size_of::<i8>();
            assert_eq!(
                unsafe {
                    libc::write(
                        self.socket_write_fd,
                        &notify_value as *const _ as *const c_void,
                        NOTIFY_VALUE_SIZE,
                    )
                },
                NOTIFY_VALUE_SIZE as isize
            );
        }

        Ok(())
    }
}

impl<P: Vst3Plugin> IPlugViewTrait for WrapperView<P> {
    #[cfg(all(target_family = "unix", not(target_os = "macos")))]
    unsafe fn isPlatformTypeSupported(&self, type_: FIDString) -> tresult {
        if unsafe { fid_matches(type_, kPlatformTypeX11EmbedWindowID) } {
            kResultOk
        } else {
            crate::nice_debug_assert_failure!("Invalid window handle type: {:?}", unsafe {
                CStr::from_ptr(type_)
            });
            kResultFalse
        }
    }

    #[cfg(target_os = "macos")]
    unsafe fn isPlatformTypeSupported(&self, type_: FIDString) -> tresult {
        if unsafe { fid_matches(type_, kPlatformTypeNSView) } {
            kResultOk
        } else {
            crate::nice_debug_assert_failure!("Invalid window handle type: {:?}", unsafe {
                CStr::from_ptr(type_)
            });
            kResultFalse
        }
    }

    #[cfg(target_os = "windows")]
    unsafe fn isPlatformTypeSupported(&self, type_: FIDString) -> tresult {
        if unsafe { fid_matches(type_, kPlatformTypeHWND) } {
            kResultOk
        } else {
            crate::nice_debug_assert_failure!("Invalid window handle type: {:?}", unsafe {
                CStr::from_ptr(type_)
            });
            kResultFalse
        }
    }

    unsafe fn attached(&self, parent: *mut c_void, type_: FIDString) -> tresult {
        check_null_ptr!(parent);

        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        let mut window = inner.editor_window.borrow_mut();
        if window.is_none() {
            let parent_handle = if unsafe { fid_matches(type_, kPlatformTypeX11EmbedWindowID) } {
                #[allow(clippy::unnecessary_cast)]
                let w = parent as usize as c_ulong;
                ParentWindowHandle::XlibWindow(w)
            } else if unsafe { fid_matches(type_, kPlatformTypeNSView) } {
                let w = NonNull::new(parent).unwrap();
                ParentWindowHandle::AppKitNsView(w)
            } else if unsafe { fid_matches(type_, kPlatformTypeHWND) } {
                let w = NonZeroIsize::new(parent as isize).unwrap();
                ParentWindowHandle::Win32Hwnd(w)
            } else {
                crate::nice_debug_assert_failure!("Unknown window handle type: {:?}", unsafe {
                    CStr::from_ptr(type_)
                });
                return kInvalidArgument;
            };

            #[derive(Debug, Clone, Copy, thiserror::Error)]
            enum ResizeError {
                #[error("Failed to post window resize task")]
                FailedToPostTask,
                #[error("Attempted to close window when plugin was closed")]
                PluginClosed,
            }

            struct Vst3HostCallbacks<P: Vst3Plugin> {
                inner: Weak<WrapperInner<P>>,
            }

            impl<P: Vst3Plugin> HostCallbacks for Vst3HostCallbacks<P> {
                fn request_resize(
                    &mut self,
                    new_size: Size,
                    scale_factor: f64,
                ) -> Result<(), Box<dyn Error>> {
                    if let Some(inner) = self.inner.upgrade() {
                        let view = inner.plug_view.read().clone().unwrap();

                        if let Err(task) = view.do_maybe_in_run_loop(Task::RequestResize {
                            size: new_size,
                            scale_factor,
                        }) && !inner.schedule_gui(task)
                        {
                            return Err(ResizeError::FailedToPostTask.into());
                        }

                        // We currently have no way to handle the host refusing to resize the
                        // window since resizing must be done in the GUI thread.
                        Ok(())
                    } else {
                        Err(ResizeError::PluginClosed.into())
                    }
                }

                fn destroyed(&mut self) {
                    // TODO: Does VST3 have a way for a plugin to notify the host that its GUI
                    // has been destroyed?
                }
            }

            let callbacks: Box<dyn HostCallbacks> = Box::new(Vst3HostCallbacks {
                inner: Arc::downgrade(&inner),
            });

            struct Vst3HostMainThreadCaller<P: Vst3Plugin> {
                inner: Weak<WrapperInner<P>>,
            }

            impl<P: Vst3Plugin> HostMainThreadCaller for Vst3HostMainThreadCaller<P> {
                fn call_main_thread(&mut self) {
                    if let Some(inner) = self.inner.upgrade() {
                        let view = inner.plug_view.read().clone().unwrap();

                        if let Err(task) = view.do_maybe_in_run_loop(Task::CallMainThread)
                            && !inner.schedule_gui(task)
                        {
                            crate::nice_error!("Failed to post Task::CallMainThread");
                        }
                    }
                }
            }

            let main_thread_caller: Box<dyn HostMainThreadCaller> =
                Box::new(Vst3HostMainThreadCaller {
                    inner: Arc::downgrade(&inner),
                });

            let fallback_scale_factor = self.fallback_scale_factor.load().map(|s| s as f64);

            match self.editor.upgrade().unwrap().lock().spawn(
                Some(parent_handle),
                false,
                fallback_scale_factor,
                inner.clone().make_gui_context(),
                Some(HostMethods {
                    callbacks,
                    main_thread_caller,
                }),
            ) {
                Ok(editor_window) => match editor_window.handle.show(&editor_window.window) {
                    Ok(()) => {
                        *window = Some(Fragile::new(editor_window));
                        inner.is_editor_open.store(true, Ordering::SeqCst);
                        kResultOk
                    }
                    Err(e) => {
                        crate::nice_error!("Failed to show editor: {}", e);
                        kResultFalse
                    }
                },
                Err(e) => {
                    crate::nice_error!("Failed to create editor: {}", e);
                    kResultFalse
                }
            }
        } else {
            #[cfg(debug_assertions)]
            crate::nice_warn!("Host tried to attach editor while the editor is already attached");

            kResultFalse
        }
    }

    unsafe fn removed(&self) -> tresult {
        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        let mut editor_handle = inner.editor_window.borrow_mut();
        if editor_handle.is_some() {
            inner.is_editor_open.store(false, Ordering::SeqCst);
            *editor_handle = None;

            kResultOk
        } else {
            #[cfg(debug_assertions)]
            crate::nice_warn!("Host tried to remove the editor without an active editor");

            kResultFalse
        }
    }

    unsafe fn onWheel(&self, _distance: f32) -> tresult {
        // We'll let the plugin use the OS' input mechanisms because not all DAWs (or very few
        // actually) implement these functions
        kNotImplemented
    }

    unsafe fn onKeyDown(&self, _key: char16, key_code: int16, modifiers: int16) -> tresult {
        self.dispatch_virtual_key(key_code, true, modifiers)
    }

    unsafe fn onKeyUp(&self, _key: char16, key_code: int16, modifiers: int16) -> tresult {
        self.dispatch_virtual_key(key_code, false, modifiers)
    }

    unsafe fn getSize(&self, size: *mut ViewRect) -> tresult {
        check_null_ptr!(size);

        let Some(editor) = self.editor.upgrade() else {
            return kResultFalse;
        };

        let size = unsafe { &mut *size };

        // TODO: This is technically incorrect during resizing, this should still report the old
        //       size until `.on_size()` has been called. We should probably only bother fixing this
        //       if it turns out to be an issue.
        let editor_size = editor.lock().size();

        size.left = 0;
        size.right = editor_size.width as i32;
        size.top = 0;
        size.bottom = editor_size.height as i32;

        kResultOk
    }

    unsafe fn onSize(&self, new_size: *mut ViewRect) -> tresult {
        check_null_ptr!(new_size);

        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        // The host is telling us the view's new frame (after honoring an earlier
        // `request_resize()`, or because the user dragged a host resize handle).
        let width = unsafe { (*new_size).right - (*new_size).left };
        let height = unsafe { (*new_size).bottom - (*new_size).top };
        if width <= 0 || height <= 0 {
            return kResultFalse;
        }

        let size = NativeSize {
            width: width as u32,
            height: height as u32,
        };

        // Apply the new size to the editor. Editors that don't support being
        // resized return `false` from `set_size()`, in which case we tell the
        // host we couldn't honor it.
        if let Some(editor_window) = inner.editor_window.borrow().as_ref() {
            let editor_window = editor_window.get();

            if let Err(e) = editor_window.handle.set_size(size, &editor_window.window) {
                crate::nice_error!("Failed to resize window to {:?}: {}", size, e);
                kResultFalse
            } else {
                kResultOk
            }
        } else {
            kResultFalse
        }
    }

    unsafe fn onFocus(&self, _state: TBool) -> tresult {
        kNotImplemented
    }

    unsafe fn setFrame(&self, frame: *mut IPlugFrame) -> tresult {
        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        #[cfg(not(all(target_family = "unix", not(target_os = "macos"))))]
        let _ = inner;

        match unsafe { ComRef::from_raw(frame) } {
            Some(frame) => {
                // On Linux the host will expose another interface that lets us run code on the
                // host's GUI thread. REAPER will segfault when we don't do this for resizes.
                #[cfg(all(target_family = "unix", not(target_os = "macos")))]
                {
                    *self.run_loop_event_handler.write() = frame
                        .cast()
                        .map(|run_loop| RunLoopEventHandler::new(Arc::downgrade(&inner), run_loop));
                }
                *self.plug_frame.write() = Some(frame.to_com_ptr());
            }
            None => {
                #[cfg(all(target_family = "unix", not(target_os = "macos")))]
                {
                    *self.run_loop_event_handler.write() = None;
                }
                *self.plug_frame.write() = None;
            }
        }

        kResultOk
    }

    unsafe fn canResize(&self) -> tresult {
        let Some(editor) = self.editor.upgrade() else {
            return kResultFalse;
        };

        // The editor decides whether it's resizable via `Editor::resize_hint()`.
        if editor.lock().resize_hint().can_resize {
            kResultOk
        } else {
            kResultFalse
        }
    }

    unsafe fn checkSizeConstraint(&self, rect: *mut ViewRect) -> tresult {
        check_null_ptr!(rect);

        let (width, height) =
            unsafe { ((*rect).right - (*rect).left, (*rect).bottom - (*rect).top) };

        if width <= 0 || height <= 0 {
            return kResultFalse;
        }

        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        let size = NativeSize {
            width: width as u32,
            height: height as u32,
        };

        if let Some(editor_window) = inner.editor_window.borrow().as_ref() {
            let editor_window = editor_window.get();

            if let Some(adjusted_size) = editor_window
                .handle
                .adjust_size(size, &editor_window.window)
            {
                if adjusted_size == size {
                    kResultOk
                } else {
                    unsafe {
                        (*rect).right = (*rect).left + adjusted_size.width as i32;
                        (*rect).bottom = (*rect).top + adjusted_size.height as i32;
                    }

                    kResultOk
                }
            } else {
                kResultFalse
            }
        } else {
            kResultFalse
        }
    }
}

impl<P: Vst3Plugin> IPlugViewContentScaleSupportTrait for WrapperView<P> {
    unsafe fn setContentScaleFactor(&self, scale_factor: ScaleFactor) -> tresult {
        let Some(inner) = self.inner.upgrade() else {
            return kResultFalse;
        };

        // TODO: So apparently Ableton Live doesn't call this function. Right now we'll hardcode the
        //       default scale to 1.0 on Linux and Windows since we can't easily get the scale from
        //       baseview. A better alternative would be to do the fallback DPI scale detection
        //       within nice-plug itself. Then we can still only use baseview's system scale policy
        //       on macOS and both the editor implementation and the wrappers would know about the
        //       correct scale.

        if !scale_factor.is_finite() || !scale_factor.is_sign_positive() {
            return kResultFalse;
        }

        if let Some(editor_window) = inner.editor_window.borrow().as_ref() {
            let editor_window = editor_window.get();
            if let Err(e) = editor_window
                .handle
                .set_fallback_scale_factor(scale_factor as f64, &editor_window.window)
            {
                crate::nice_error!("Failed to set suggested scale factor: {}", e);
                kResultFalse
            } else {
                self.fallback_scale_factor.store(Some(scale_factor));
                kResultOk
            }
        } else {
            kResultFalse
        }
    }
}

#[cfg(all(target_family = "unix", not(target_os = "macos")))]
impl<P: Vst3Plugin> IEventHandlerTrait for RunLoopEventHandler<P> {
    unsafe fn onFDIsSet(&self, _fd: FileDescriptor) {
        // There should be a one-to-one correlation to bytes being written to `self.socket_read_fd`
        // and events being pushed to `self.tasks`, but because the process of pushing a task and
        // notifying this thread through the socket is not atomic we can't reliably just read a byte
        // from this socket for every task we process. For instance, if `Self::post_task()` gets
        // called while this loop is already running, it could happen that we pop and execute the
        // task before the byte gets written to the socket. To avoid this, we'll clear the socket
        // upfront, and then execute the tasks afterwards. If this situation does happen, then the
        // worst thing that can happen is that this function is called a second time while
        // `self.tasks()` is already empty.
        let mut notify_value = [0; 32];
        loop {
            let read_result = unsafe {
                libc::read(
                    self.socket_read_fd,
                    &mut notify_value as *mut _ as *mut c_void,
                    std::mem::size_of_val(&notify_value),
                )
            };

            // If after the first loop the socket contains no more data, then the `read()` call will
            // return -1 and `errno` will have been set to `EAGAIN`
            if read_result <= 0 {
                break;
            }
        }

        self.notified.store(false, Ordering::SeqCst);

        // This gets called from the host's UI thread because we wrote some bytes to the Unix domain
        // socket. We'll read that data from the socket again just to make REAPER happy.
        while let Some(task) = self.tasks.pop() {
            if let Some(inner) = self.inner.upgrade() {
                inner.execute(task, true);
            }
        }
    }
}

#[cfg(all(target_family = "unix", not(target_os = "macos")))]
impl<P: Vst3Plugin> Drop for RunLoopEventHandler<P> {
    // PLEASE FIXME: This drop method never actually gets called for some reason.
    fn drop(&mut self) {
        // When this object gets dropped and there are still unprocessed tasks left, then we'll
        // handle those in the regular event loop so no work gets lost
        let mut posting_failed = false;
        while let Some(task) = self.tasks.pop() {
            if let Some(inner) = self.inner.upgrade() {
                posting_failed |= !inner
                    .event_loop
                    .borrow()
                    .as_ref()
                    .unwrap()
                    .schedule_gui(task);
            }
        }

        if posting_failed {
            crate::nice_debug_assert_failure!(
                "Outstanding tasks have been dropped when closing the editor as the task queue \
                 was full"
            );
        }

        unsafe {
            libc::close(self.socket_read_fd);
            libc::close(self.socket_write_fd);
        }

        unsafe {
            self.run_loop
                .unregisterEventHandler(self.event_handler_ptr.0.get());
        }
    }
}
