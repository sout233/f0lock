use super::*;
use clap_sys::ext::{latency::*, tail::*};

pub(super) const HOST_LATENCY: clap_host_latency = clap_host_latency {
    changed: Some(notify),
};

unsafe fn process_sample(
    plugin: *const clap_plugin,
    input: [f32; 2],
    change: Option<(u32, f64)>,
) -> [f32; 2] {
    unsafe {
        let mut input = input;
        let mut sidechain = [0.0_f32; 2];
        let mut output = [f32::NAN; 2];
        let mut input_channels = [input.as_mut_ptr(), input.as_mut_ptr().add(1)];
        let mut sc_channels = [sidechain.as_mut_ptr(), sidechain.as_mut_ptr().add(1)];
        let mut output_channels = [output.as_mut_ptr(), output.as_mut_ptr().add(1)];
        let main = clap_audio_buffer {
            data32: input_channels.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let inputs = [
            main,
            clap_audio_buffer {
                data32: sc_channels.as_mut_ptr(),
                ..main
            },
        ];
        let mut outputs = clap_audio_buffer {
            data32: output_channels.as_mut_ptr(),
            ..main
        };
        let mut event = change.map(|(param_id, value)| clap_event_param_value {
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
        let events = clap_input_events {
            ctx: event.as_mut().map_or(ptr::null_mut(), |e| {
                (e as *mut clap_event_param_value).cast()
            }),
            size: Some(event_count),
            get: Some(event_get),
        };
        let out_events = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(output_push),
        };
        let process = clap_process {
            steady_time: -1,
            frames_count: 1,
            transport: ptr::null(),
            audio_inputs: inputs.as_ptr(),
            audio_outputs: &mut outputs,
            audio_inputs_count: 2,
            audio_outputs_count: 1,
            in_events: &events,
            out_events: &out_events,
        };
        assert_ne!(
            (*plugin).process.unwrap()(plugin, &process),
            CLAP_PROCESS_ERROR
        );
        assert!(output.iter().all(|v| v.is_finite()));
        output
    }
}

/// Runs after state restore while inactive. The restored Lookahead setting
/// must affect activation, actual impulse timing, tail reporting and toggling.
pub(super) unsafe fn verify(plugin: *const clap_plugin, params: *const clap_plugin_params) {
    unsafe {
        let latency = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_LATENCY.as_ptr())
            .cast::<clap_plugin_latency>();
        let tail = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_TAIL.as_ptr())
            .cast::<clap_plugin_tail>();
        assert!(!latency.is_null() && !tail.is_null());
        assert!((*plugin).activate.unwrap()(plugin, 48000.0, 1, 512));
        assert_eq!((*latency).get.unwrap()(plugin), 7200);
        (*plugin).on_main_thread.unwrap()(plugin);
        assert!((*plugin).start_processing.unwrap()(plugin));
        (*plugin).reset.unwrap()(plugin);

        for i in 0..7232 {
            let input = if i == 7 { [0.25, -0.25] } else { [0.0; 2] };
            let event = (i == 0).then(|| text_event(plugin, params, "Bypass", "On"));
            let output = process_sample(plugin, input, event);
            let expected = if i == 7207 { [0.25, -0.25] } else { [0.0; 2] };
            assert_eq!(output, expected, "Lookahead impulse at sample {i}");
        }

        assert_eq!((*tail).get.unwrap()(plugin), 7200);
        let off = text_event(plugin, params, "Lookahead", "Off");
        assert_eq!(
            process_sample(plugin, [0.25, -0.25], Some(off)),
            [0.25, -0.25]
        );
        assert_eq!((*latency).get.unwrap()(plugin), 0);
        (*plugin).stop_processing.unwrap()(plugin);
        (*plugin).on_main_thread.unwrap()(plugin);
        (*plugin).deactivate.unwrap()(plugin);
        println!("Lookahead state / latency / impulse / bypass / tail / runtime disable passed");
    }
}
