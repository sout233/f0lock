use crate::{Telemetry, editor::F0Editor, params::F0Params};
use nice_plug::{context::gui::GuiContext, editor::*};
use nice_plug_egui::{EguiEditorState, NiceEguiApp, RepaintNotifier};
use std::{
    num::NonZeroIsize,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

struct Probe {
    editor: F0Editor,
    metrics: Arc<Mutex<Vec<(f32, egui::Vec2)>>>,
    #[cfg(feature = "dpi-trace")]
    configurations: Arc<std::sync::atomic::AtomicU64>,
}

impl NiceEguiApp for Probe {
    fn build(
        &mut self,
        ctx: egui::Context,
        gui: GuiContext,
        frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.editor.build(ctx, gui, frame)
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut nice_plug_egui::Frame) {
        #[cfg(feature = "dpi-trace")]
        self.configurations.store(
            frame.gpu_counters().configurations,
            std::sync::atomic::Ordering::Relaxed,
        );
        self.metrics
            .lock()
            .unwrap()
            .push((ui.ctx().pixels_per_point(), ui.ctx().content_rect().size()));
        self.editor.ui(ui, frame);
    }

    fn editor_closed(&mut self) {
        self.editor.editor_closed();
    }
}

struct Parent(HWND);

struct HostResize {
    parent: HWND,
    calls: Arc<Mutex<Vec<dpi::NativeSize<u32>>>>,
}

impl HostCallbacks for HostResize {
    fn request_resize(
        &mut self,
        size: dpi::Size,
        scale: f64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let size = dpi::NativeSize::<u32>::from_size(size, scale);
        self.calls.lock().unwrap().push(size);
        let previous =
            unsafe { SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(self.parent)) };
        assert!(!previous.is_null());
        let resized = unsafe {
            SetWindowPos(
                self.parent,
                std::ptr::null_mut(),
                0,
                0,
                size.width as i32,
                size.height as i32,
                SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
        let error = std::io::Error::last_os_error();
        assert!(!unsafe { SetThreadDpiAwarenessContext(previous) }.is_null());
        if resized == 0 {
            return Err(error.into());
        }
        Ok(())
    }

    fn destroyed(&mut self) {
        panic!("Plugin destroyed its native window unexpectedly");
    }
}

struct MainThread;

impl HostMainThreadCaller for MainThread {
    fn call_main_thread(&mut self) {}
}

impl Parent {
    fn new(size: dpi::NativeSize<u32>) -> Self {
        let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
        let hwnd = unsafe {
            CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                size.width as i32,
                size.height as i32,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        assert!(!hwnd.is_null(), "{}", std::io::Error::last_os_error());
        Self(hwnd)
    }
}

impl Drop for Parent {
    fn drop(&mut self) {
        assert_ne!(unsafe { DestroyWindow(self.0) }, 0);
    }
}

#[test]
#[ignore = "creates native Windows plugin windows; run with --ignored --nocapture --test-threads=1"]
fn native_editor_reopen_keeps_host_size_and_content_scale_consistent() {
    let process_dpi = match std::env::var("F0LOCK_NATIVE_PROCESS_DPI").as_deref() {
        Ok("unaware") => DPI_AWARENESS_CONTEXT_UNAWARE,
        Ok("aware") | Err(_) => DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        Ok(other) => panic!("Unsupported native test DPI mode: {other}"),
    };
    let parent_dpi = match std::env::var("F0LOCK_NATIVE_PARENT_DPI").as_deref() {
        Ok("unaware") => DPI_AWARENESS_CONTEXT_UNAWARE,
        Ok("aware") | Err(_) => DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        Ok(other) => panic!("Unsupported native parent DPI mode: {other}"),
    };
    assert_ne!(unsafe { SetProcessDpiAwarenessContext(process_dpi) }, 0);
    assert!(
        !unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
            .is_null()
    );
    let params = Arc::new(F0Params::default());
    let metrics = Arc::new(Mutex::new(Vec::new()));
    #[cfg(feature = "dpi-trace")]
    let configurations = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let editor = nice_plug_egui::create_egui_editor(
        EguiEditorState::from_size(dpi::LogicalSize::new(900.0, 684.0), 1.0),
        RepaintNotifier::new(),
        crate::editor_settings(),
        Probe {
            editor: F0Editor::new(params.clone(), Arc::new(Telemetry::default())),
            metrics: metrics.clone(),
            #[cfg(feature = "dpi-trace")]
            configurations: configurations.clone(),
        },
    )
    .unwrap();

    let host = Arc::new(super::RecordingHost::default());
    let mut samples = Vec::new();

    for reopen in 0..4 {
        if reopen == 2 {
            params
                .ui_scale_percent
                .store(125, std::sync::atomic::Ordering::Relaxed);
        }
        assert!(!unsafe { SetThreadDpiAwarenessContext(parent_dpi) }.is_null());
        let parent = Parent::new(editor.size());
        if reopen > 0 {
            assert!(
                !unsafe {
                    SetThreadDpiAwarenessContext(if parent_dpi == DPI_AWARENESS_CONTEXT_UNAWARE {
                        DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
                    } else {
                        DPI_AWARENESS_CONTEXT_UNAWARE
                    })
                }
                .is_null()
            );
        }
        let calls = Arc::new(Mutex::new(Vec::new()));
        metrics.lock().unwrap().clear();
        let spawned = editor
            .spawn(
                Some(ParentWindowHandle::Win32Hwnd(
                    NonZeroIsize::new(parent.0 as isize).unwrap(),
                )),
                false,
                None,
                GuiContext::new(host.clone()),
                Some(HostMethods {
                    callbacks: Box::new(HostResize {
                        parent: parent.0,
                        calls: calls.clone(),
                    }),
                    main_thread_caller: Box::new(MainThread),
                }),
            )
            .unwrap();
        spawned.handle.show(&spawned.window).unwrap();
        let child = unsafe { GetWindow(parent.0, GW_CHILD) };
        assert!(!child.is_null());
        unsafe {
            ShowWindow(parent.0, SW_SHOW);
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(child, std::ptr::null(), 0);
            windows_sys::Win32::Graphics::Gdi::UpdateWindow(child);
        }

        let end = Instant::now() + Duration::from_millis(250);
        while Instant::now() < end {
            let mut msg = MSG::default();
            if unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
                unsafe {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            } else {
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        let size = spawned.window.size();
        #[cfg(feature = "dpi-trace")]
        if reopen == 0 {
            assert_eq!(
                configurations.load(std::sync::atomic::Ordering::Relaxed),
                1,
                "Unchanged native window reconfigured the GPU surface between frames"
            );
        }
        assert_eq!(
            unsafe { GetDpiForWindow(child) },
            unsafe { GetDpiForWindow(parent.0) },
            "Plugin child uses process DPI instead of host parent DPI"
        );
        let reported = editor.size();
        let mut parent_rect = windows_sys::Win32::Foundation::RECT::default();
        let previous =
            unsafe { SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(parent.0)) };
        assert!(!previous.is_null());
        assert_ne!(unsafe { GetClientRect(parent.0, &mut parent_rect) }, 0);
        assert!(!unsafe { SetThreadDpiAwarenessContext(previous) }.is_null());
        assert_eq!(
            (parent_rect.right as u32, parent_rect.bottom as u32),
            (size.physical.width, size.physical.height),
            "Host parent did not follow plugin DPI/zoom resize; calls={:?}",
            calls.lock().unwrap()
        );
        println!(
            "native window alive={}, visible={}, size={size:?}, child={child:?}",
            spawned.window.is_open(),
            unsafe { IsWindowVisible(child) }
        );
        let metric = *metrics
            .lock()
            .unwrap()
            .last()
            .expect("No native frame rendered");
        println!(
            "reopen={reopen}, window={size:?}, reported={reported:?}, content={metric:?}, thread_dpi={:?}",
            unsafe { GetThreadDpiAwarenessContext() }
        );
        samples.push((
            size,
            reported,
            metric,
            params
                .ui_scale_percent
                .load(std::sync::atomic::Ordering::Relaxed),
        ));
        drop(spawned);
        assert_eq!(
            editor.size(),
            reported,
            "Closing discarded the physical size needed for reopening"
        );
        drop(parent);
    }

    for (size, reported, metric, percent) in &samples {
        assert_eq!(
            (size.physical.width, size.physical.height),
            (reported.width, reported.height),
            "Native child and host dimensions disagree: {samples:?}"
        );
        assert!(
            (metric.1.x - 900.0).abs() < 1.0 && (metric.1.y - 684.0).abs() < 1.0,
            "UI cropped or surrounded by empty space: {samples:?}"
        );
        assert!(
            (metric.0 - size.scale_factor as f32 * *percent as f32 / 100.0).abs() < 0.001,
            "Reopen changed saved content scale: {samples:?}"
        );
    }

    let opengl: Vec<u16> = "opengl32.dll\0".encode_utf16().collect();
    assert!(
        unsafe { windows_sys::Win32::System::LibraryLoader::GetModuleHandleW(opengl.as_ptr()) }
            .is_null(),
        "Windows editor loaded OpenGL despite selecting D3D12"
    );
}
