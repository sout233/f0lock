//! Loads the packaged DLL through its public CLAP ABI.
//! Run after bundling: cargo test --test clap_host -- --ignored --nocapture
use clap_sys::{
    audio_buffer::*, entry::*, events::*, ext::params::*, factory::plugin_factory::*, host::*,
    plugin::*, process::*, version::*,
};
use clap_sys::{ext::state::*, stream::*};
use std::io::{Cursor, Read};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    ptr,
};

#[cfg(target_os = "windows")]
#[path = "support/clap_gui.rs"]
mod gui_tests;

fn bundle_path(relative: &str) -> std::path::PathBuf {
    let directory = std::env::var_os("F0LOCK_TEST_BUNDLE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/bundled"));
    directory.join(relative)
}

unsafe extern "C" fn write_state(
    stream: *const clap_ostream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    unsafe {
        let bytes = &mut *(*stream).ctx.cast::<Vec<u8>>();
        bytes.extend_from_slice(std::slice::from_raw_parts(
            buffer.cast::<u8>(),
            size as usize,
        ));
    }
    size as i64
}

unsafe extern "C" fn read_state(
    stream: *const clap_istream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    unsafe {
        let bytes = &mut *(*stream).ctx.cast::<Cursor<Vec<u8>>>();
        bytes
            .read(std::slice::from_raw_parts_mut(
                buffer.cast::<u8>(),
                size as usize,
            ))
            .unwrap() as i64
    }
}

unsafe fn saved_band_state_round_trip(
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
) {
    unsafe {
        let mut expected = Vec::new();
        let out = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(output_push),
        };

        for (name, text) in [
            ("Pitch Detection", "pYIN"),
            ("Target Mode", "Reference"),
            ("Target Minimum", "150 Hz"),
            ("Target Maximum", "700 Hz"),
            ("Target Reference", "350 Hz"),
            ("Target Tolerance", "7 st"),
            ("Attack", "25 ms"),
            ("Release", "250 ms"),
            ("Band 2 exists", "On"),
            ("Band 2 ratio", "2.5"),
            ("Band 2 type", "High Shelf"),
            ("Band 2 gain", "4.5 dB"),
        ] {
            let mut selected = None;

            for i in 0..(*params).count.unwrap()(plugin) {
                let mut info = std::mem::zeroed::<clap_param_info>();

                assert!((*params).get_info.unwrap()(plugin, i, &mut info));

                if CStr::from_ptr(info.name.as_ptr()).to_str().unwrap() == name {
                    selected = Some(info.id);
                }
            }

            let id = selected.expect("Missing additional-band parameter");

            let text = CString::new(text).unwrap();

            let mut value = 0.0;

            assert!(
                (*params).text_to_value.unwrap()(plugin, id, text.as_ptr(), &mut value),
                "{name}"
            );

            let mut event = clap_event_param_value {
                header: clap_event_header {
                    size: size_of::<clap_event_param_value>() as u32,
                    time: 0,
                    space_id: 0,
                    type_: CLAP_EVENT_PARAM_VALUE,
                    flags: 0,
                },
                param_id: id,
                cookie: ptr::null_mut(),
                note_id: -1,
                port_index: -1,
                channel: -1,
                key: -1,
                value,
            };

            let events = clap_input_events {
                ctx: (&mut event as *mut clap_event_param_value).cast(),
                size: Some(event_count),
                get: Some(event_get),
            };
            (*params).flush.unwrap()(plugin, &events, &out);
            let mut actual = 0.0;

            assert!((*params).get_value.unwrap()(plugin, id, &mut actual));
            assert!((actual - value).abs() < 1e-6);
            expected.push((id, value, event));
        }

        let state = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_STATE.as_ptr())
            .cast::<clap_plugin_state>();

        assert!(!state.is_null());
        let mut saved = Vec::<u8>::new();
        let writer = clap_ostream {
            ctx: (&mut saved as *mut Vec<u8>).cast(),
            write: Some(write_state),
        };

        assert!((*state).save.unwrap()(plugin, &writer));
        assert!(!saved.is_empty());

        for (_, _, event) in &mut expected {
            event.value = 0.0;
            let events = clap_input_events {
                ctx: (event as *mut clap_event_param_value).cast(),
                size: Some(event_count),
                get: Some(event_get),
            };
            (*params).flush.unwrap()(plugin, &events, &out);
        }

        let mut saved = Cursor::new(saved);
        let reader = clap_istream {
            ctx: (&mut saved as *mut Cursor<Vec<u8>>).cast(),
            read: Some(read_state),
        };

        assert!((*state).load.unwrap()(plugin, &reader));

        for (id, expected, _) in expected {
            let mut actual = 0.0;

            assert!((*params).get_value.unwrap()(plugin, id, &mut actual));
            assert!(
                (actual - expected).abs() < 1e-6,
                "State restore mismatch for {id}"
            );
        }

        println!("Pitch detection and additional band parameters restored from CLAP state");
    }
}

unsafe extern "C" fn extension(_: *const clap_host, _: *const c_char) -> *const c_void {
    ptr::null()
}
unsafe extern "C" fn notify(_: *const clap_host) {}
unsafe extern "C" fn event_count(list: *const clap_input_events) -> u32 {
    u32::from(unsafe { !(*list).ctx.is_null() })
}
unsafe extern "C" fn event_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    if index == 0 {
        unsafe { (*list).ctx.cast() }
    } else {
        ptr::null()
    }
}
unsafe extern "C" fn output_push(
    _: *const clap_output_events,
    _: *const clap_event_header,
) -> bool {
    true
}

/// Supply a real tone in variable-size host buffers and measure the steady-state result.
unsafe fn render(plugin: *const clap_plugin, gain_event: Option<(u32, f64)>) -> f64 {
    let mut event = gain_event.map(|(param_id, value)| clap_event_param_value {
        header: clap_event_header {
            size: size_of::<clap_event_param_value>() as u32,
            time: 0,
            space_id: 0,
            type_: CLAP_EVENT_PARAM_VALUE,
            flags: 0,
        },
        param_id,
        cookie: ptr::null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value,
    });

    let mut events = clap_input_events {
        ctx: event.as_mut().map_or(ptr::null_mut(), |e| {
            (e as *mut clap_event_param_value).cast()
        }),
        size: Some(event_count),
        get: Some(event_get),
    };

    let output_events = clap_output_events {
        ctx: ptr::null_mut(),
        try_push: Some(output_push),
    };

    let mut power_in = 0.0_f64;
    let mut power_out = 0.0_f64;
    let mut total = 0;
    let mut block = 0;

    while total < 48000 {
        let frames = [1, 64, 127, 256, 511][block % 5].min(48000 - total);
        let mut left = [0.0_f32; 512];
        let mut right = [0.0_f32; 512];
        let mut out_left = [f32::NAN; 512];
        let mut out_right = [f32::NAN; 512];

        for i in 0..frames {
            left[i] = 0.1 * (std::f32::consts::TAU * 220.0 * (total + i) as f32 / 48000.0).sin();
            right[i] = -left[i];
        }

        let mut inputs = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut outputs = [out_left.as_mut_ptr(), out_right.as_mut_ptr()];
        let input = clap_audio_buffer {
            data32: inputs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };

        let mut output = clap_audio_buffer {
            data32: outputs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };

        let process = clap_process {
            steady_time: total as i64,
            frames_count: frames as u32,
            transport: ptr::null(),
            audio_inputs: &input,
            audio_outputs: &mut output,
            audio_inputs_count: 1,
            audio_outputs_count: 1,
            in_events: &events,
            out_events: &output_events,
        };

        let status = unsafe { (*plugin).process.unwrap()(plugin, &process) };

        assert_ne!(status, CLAP_PROCESS_ERROR);
        events.ctx = ptr::null_mut();

        for i in 0..frames {
            assert!(out_left[i].is_finite() && out_right[i].is_finite());
            assert!((out_left[i] + out_right[i]).abs() < 1e-6, "Stereo mismatch");

            if total + i >= 24000 {
                power_in += f64::from(left[i]).powi(2);
                power_out += f64::from(out_left[i]).powi(2);
            }
        }
        total += frames;
        block += 1;
    }
    10.0 * (power_out / power_in).log10()
}

#[test]
#[ignore = "requires cargo xtask bundle f0lock --release"]
fn packaged_clap_processes_and_accepts_host_automation() {
    let path = bundle_path("f0lock.clap");
    let path_c = CString::new(path.to_str().unwrap()).unwrap();
    unsafe {
        let library = libloading::Library::new(&path).expect("Build the release CLAP bundle first");
        let symbol: libloading::Symbol<*const clap_plugin_entry> =
            library.get(b"clap_entry\0").unwrap();
        let entry = &**symbol;

        assert!(entry.init.unwrap()(path_c.as_ptr()));
        let factory = entry.get_factory.unwrap()(CLAP_PLUGIN_FACTORY_ID.as_ptr())
            .cast::<clap_plugin_factory>();

        assert!(!factory.is_null());
        assert_eq!((*factory).get_plugin_count.unwrap()(factory), 1);
        let descriptor = (*factory).get_plugin_descriptor.unwrap()(factory, 0);

        assert_eq!(
            CStr::from_ptr((*descriptor).name).to_str().unwrap(),
            "f0lock"
        );
        let host = clap_host {
            clap_version: CLAP_VERSION,
            host_data: ptr::null_mut(),
            name: c"f0lock integration test".as_ptr(),
            vendor: c"f0lock".as_ptr(),
            url: c"".as_ptr(),
            version: c"1".as_ptr(),
            get_extension: Some(extension),
            request_restart: Some(notify),
            request_process: Some(notify),
            request_callback: Some(notify),
        };

        let plugin = (*factory).create_plugin.unwrap()(factory, &host, (*descriptor).id);

        assert!(!plugin.is_null());
        assert!((*plugin).init.unwrap()(plugin));
        let params = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_PARAMS.as_ptr())
            .cast::<clap_plugin_params>();

        assert!(!params.is_null());
        let count = (*params).count.unwrap()(plugin);
        assert_eq!(count, 62);
        let mut gain_id = None;
        let mut bypass_id = None;

        for i in 0..count {
            let mut info = std::mem::zeroed::<clap_param_info>();

            assert!((*params).get_info.unwrap()(plugin, i, &mut info));

            match CStr::from_ptr(info.name.as_ptr()).to_str().unwrap() {
                "F0 gain" => gain_id = Some(info.id),
                "Bypass" => bypass_id = Some(info.id),
                _ => {}
            }
        }

        assert!((*plugin).activate.unwrap()(plugin, 48000.0, 1, 512));
        assert!((*plugin).start_processing.unwrap()(plugin));
        let cut = render(plugin, None);

        assert!((cut + 6.0).abs() < 0.1, "Default gain: {cut}");
        let mut boost_value = 0.0;

        assert!((*params).text_to_value.unwrap()(
            plugin,
            gain_id.unwrap(),
            c"12 dB".as_ptr(),
            &mut boost_value
        ));
        let boost = render(plugin, Some((gain_id.unwrap(), boost_value)));

        assert!((boost - 12.0).abs() < 0.1, "Automated gain: {boost}");
        let bypass = render(plugin, Some((bypass_id.unwrap(), 1.0)));

        assert!(bypass.abs() < 1e-6, "Bypass: {bypass}");

        println!(
            "Loaded CLAP: default {cut:.3} dB, automated {boost:.3} dB, bypass {bypass:.6} dB"
        );
        (*plugin).stop_processing.unwrap()(plugin);
        (*plugin).deactivate.unwrap()(plugin);
        saved_band_state_round_trip(plugin, params);
        (*plugin).destroy.unwrap()(plugin);
        entry.deinit.unwrap()();
    }
}

#[test]
#[cfg(target_os = "windows")]
#[ignore = "requires cargo xtask bundle f0lock --release"]
fn packaged_vst3_exposes_a_loadable_factory() {
    let path = bundle_path("f0lock.vst3/Contents/x86_64-win/f0lock.vst3");
    unsafe {
        let library = libloading::Library::new(path).expect("Build the VST3 bundle first");
        let init: libloading::Symbol<unsafe extern "system" fn() -> bool> =
            library.get(b"InitDll\0").unwrap();
        let exit: libloading::Symbol<unsafe extern "system" fn() -> bool> =
            library.get(b"ExitDll\0").unwrap();
        let get_factory: libloading::Symbol<unsafe extern "system" fn() -> *mut c_void> =
            library.get(b"GetPluginFactory\0").unwrap();

        assert!(init());
        let factory = get_factory();

        assert!(!factory.is_null());
        // Release the IPluginFactory through its FUnknown vtable.
        let vtable = *factory.cast::<*const *const c_void>();
        let release: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));
        release(factory);

        assert!(exit());

        println!("Loaded VST3: InitDll / GetPluginFactory / ExitDll passed");
    }
}
