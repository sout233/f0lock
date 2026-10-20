use super::*;
use clap_sys::ext::gui::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use windows_sys::Win32::{
    Foundation::HWND,
    UI::{HiDpi::*, WindowsAndMessaging::*},
};

#[path = "clap_audio.rs"]
mod audio;

struct GuiHost {
    parent: HWND,
    callback_requested: AtomicBool,
}

unsafe extern "C" fn gui_extension(_: *const clap_host, id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_EXT_GUI {
        (&HOST_GUI as *const clap_host_gui).cast()
    } else {
        ptr::null()
    }
}

unsafe extern "C" fn resize(host: *const clap_host, width: u32, height: u32) -> bool {
    let host = unsafe { &*(*host).host_data.cast::<GuiHost>() };
    let previous =
        unsafe { SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(host.parent)) };
    if previous.is_null() {
        return false;
    }
    let result = unsafe {
        SetWindowPos(
            host.parent,
            ptr::null_mut(),
            0,
            0,
            width as i32,
            height as i32,
            SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOZORDER,
        )
    } != 0;
    unsafe { SetThreadDpiAwarenessContext(previous) };
    result
}

unsafe extern "C" fn callback(host: *const clap_host) {
    unsafe { &*(*host).host_data.cast::<GuiHost>() }
        .callback_requested
        .store(true, Ordering::Release);
}

static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: None,
    request_resize: Some(resize),
    request_show: None,
    request_hide: None,
    closed: None,
};

#[test]
#[ignore = "requires the release bundle and creates native GUI windows"]
fn packaged_clap_reopens_native_gui_without_changing_scale_or_host_size() {
    let path = bundle_path("f0lock.clap");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    let parent_dpi = match std::env::var("F0LOCK_CLAP_PARENT_DPI").as_deref() {
        Ok("v1") => DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE,
        Ok("system") => DPI_AWARENESS_CONTEXT_SYSTEM_AWARE,
        Ok("unaware") => DPI_AWARENESS_CONTEXT_UNAWARE,
        Ok("v2") | Err(_) => DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        Ok(other) => panic!("Unsupported CLAP parent DPI mode: {other}"),
    };
    let host_scale = std::env::var("F0LOCK_CLAP_HOST_SCALE")
        .ok()
        .map(|text| text.parse::<f64>().unwrap());
    let soak_seconds = std::env::var("F0LOCK_CLAP_SOAK_SECONDS")
        .ok()
        .map(|text| text.parse::<u64>().unwrap());
    unsafe {
        let previous_dpi = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        assert!(!previous_dpi.is_null());
        let library = libloading::Library::new(&path).unwrap();
        let symbol: libloading::Symbol<*const clap_plugin_entry> =
            library.get(b"clap_entry\0").unwrap();
        let entry = &**symbol;
        assert!(entry.init.unwrap()(path_c.as_ptr()));
        let factory = entry.get_factory.unwrap()(CLAP_PLUGIN_FACTORY_ID.as_ptr())
            .cast::<clap_plugin_factory>();
        assert!(!factory.is_null());
        let descriptor = (*factory).get_plugin_descriptor.unwrap()(factory, 0);
        let mut data = GuiHost {
            parent: ptr::null_mut(),
            callback_requested: AtomicBool::new(false),
        };
        let host = clap_host {
            clap_version: CLAP_VERSION,
            host_data: (&mut data as *mut GuiHost).cast(),
            name: c"Native GUI regression host".as_ptr(),
            vendor: c"f0lock".as_ptr(),
            url: c"".as_ptr(),
            version: c"1".as_ptr(),
            get_extension: Some(gui_extension),
            request_restart: Some(notify),
            request_process: Some(notify),
            request_callback: Some(callback),
        };
        let plugin = (*factory).create_plugin.unwrap()(factory, &host, (*descriptor).id);
        assert!(!plugin.is_null());
        assert!((*plugin).init.unwrap()(plugin));
        let gui = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_GUI.as_ptr())
            .cast::<clap_plugin_gui>();
        assert!(!gui.is_null());
        let mut observations = Vec::new();
        let mut orphan_timers = std::collections::HashSet::new();
        let mut paint_counts = Vec::new();
        assert!((*plugin).activate.unwrap()(plugin, 48000.0, 1, 512));
        let stop_audio = Arc::new(AtomicBool::new(false));
        let audio_stop = stop_audio.clone();
        let plugin_address = plugin as usize;
        let audio_thread = std::thread::spawn(move || audio::run(plugin_address, audio_stop));

        for reopen in 0..4 {
            SetThreadDpiAwarenessContext(parent_dpi);
            assert!((*gui).create.unwrap()(
                plugin,
                CLAP_WINDOW_API_WIN32.as_ptr(),
                false
            ));
            if let Some(scale) = host_scale {
                assert!((*gui).set_scale.unwrap()(plugin, scale));
            }
            let (mut width, mut height) = (0, 0);
            assert!((*gui).get_size.unwrap()(plugin, &mut width, &mut height));
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            data.parent = CreateWindowExW(
                0,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                0,
                0,
                width as i32,
                height as i32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );
            assert!(!data.parent.is_null());
            if reopen > 0 {
                SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE);
            }
            let parent = clap_window {
                api: CLAP_WINDOW_API_WIN32.as_ptr(),
                specific: clap_window_handle { win32: data.parent },
            };
            assert!((*gui).set_parent.unwrap()(plugin, &parent));
            assert!((*gui).show.unwrap()(plugin));
            let child = GetWindow(data.parent, GW_CHILD);
            assert!(!child.is_null());
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(child, ptr::null(), 0);
            windows_sys::Win32::Graphics::Gdi::UpdateWindow(child);

            let duration = if reopen == 0 {
                soak_seconds
                    .map(Duration::from_secs)
                    .unwrap_or(Duration::from_millis(250))
            } else {
                Duration::from_millis(250)
            };
            let end = Instant::now() + duration;
            let mut paints = 0_u64;
            let mut interval_started = Instant::now();
            let mut seconds_open = 0_u64;
            while Instant::now() < end {
                let mut message = MSG::default();
                if PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
                    if message.message == WM_TIMER && message.hwnd.is_null() {
                        orphan_timers.insert(message.wParam);
                    }
                    if message.message == WM_PAINT && message.hwnd == child {
                        paints += 1;
                    }
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
                if data.callback_requested.swap(false, Ordering::AcqRel) {
                    (*plugin).on_main_thread.unwrap()(plugin);
                }
                if interval_started.elapsed() >= Duration::from_secs(1) {
                    paint_counts.push(paints);
                    paints = 0;
                    interval_started = Instant::now();
                    seconds_open += 1;
                    if seconds_open.is_multiple_of(30) {
                        let position = ((height as f64 * 0.65) as isize) << 16
                            | (width as f64 * 0.12) as isize;
                        for (message, flags) in
                            [(WM_MOUSEMOVE, 0), (WM_LBUTTONDOWN, 1), (WM_LBUTTONUP, 0)]
                        {
                            assert_ne!(PostMessageW(child, message, flags, position), 0);
                        }
                        println!(
                            "GUI soak: {seconds_open}s, last paint count={}",
                            paint_counts.last().unwrap()
                        );
                    }
                }
            }

            assert!(
                IsWindow(child) != 0,
                "Native GUI disappeared on reopen {reopen}"
            );
            assert!((*gui).get_size.unwrap()(plugin, &mut width, &mut height));
            let old = SetThreadDpiAwarenessContext(GetWindowDpiAwarenessContext(child));
            assert!(!old.is_null());
            let mut child_rect = windows_sys::Win32::Foundation::RECT::default();
            let mut host_rect = windows_sys::Win32::Foundation::RECT::default();
            assert_ne!(GetClientRect(child, &mut child_rect), 0);
            assert_ne!(GetClientRect(data.parent, &mut host_rect), 0);
            SetThreadDpiAwarenessContext(old);
            observations.push((
                (width, height),
                (child_rect.right as u32, child_rect.bottom as u32),
                (host_rect.right as u32, host_rect.bottom as u32),
            ));
            assert_eq!(GetDpiForWindow(child), GetDpiForWindow(data.parent));
            println!(
                "Release CLAP GUI reopen={reopen}: reported={width}x{height}, child={}x{}, host={}x{}, DPI={}",
                child_rect.right,
                child_rect.bottom,
                host_rect.right,
                host_rect.bottom,
                GetDpiForWindow(child)
            );
            assert!((*gui).hide.unwrap()(plugin));
            (*gui).destroy.unwrap()(plugin);
            assert_eq!(
                IsWindow(child),
                0,
                "Closing retained an old native child window"
            );
            assert_ne!(DestroyWindow(data.parent), 0);
            data.parent = ptr::null_mut();
        }

        stop_audio.store(true, Ordering::Release);
        let blocks = audio_thread.join().unwrap();
        (*plugin).deactivate.unwrap()(plugin);
        let active_orphan_timers = orphan_timers
            .iter()
            .filter(|id| KillTimer(ptr::null_mut(), **id) != 0)
            .count();
        (*plugin).destroy.unwrap()(plugin);
        entry.deinit.unwrap()();
        assert!(!SetThreadDpiAwarenessContext(previous_dpi).is_null());
        println!(
            "Audio blocks={blocks}, paints/second={paint_counts:?}, thread timer IDs observed={}, still active after closing={active_orphan_timers}",
            orphan_timers.len()
        );
        assert_eq!(
            active_orphan_timers, 0,
            "Native GUI leaked active thread timers"
        );
        assert!(
            paint_counts.iter().all(|count| *count > 0),
            "Native GUI stopped repainting: {paint_counts:?}"
        );
        for (reported, child, parent) in &observations {
            assert_eq!(
                reported, child,
                "Reported and native size disagree: {observations:?}"
            );
            assert_eq!(
                reported, parent,
                "Reported and host size disagree: {observations:?}"
            );
            assert_eq!(
                *reported, observations[0].0,
                "Reopening changed GUI scale: {observations:?}"
            );
        }
    }
}
