use super::*;
use clap_sys::ext::{latency::*, note_ports::*};

unsafe fn set_param(
    plugin: *const clap_plugin,
    params: *const clap_plugin_params,
    name: &str,
    text: &str,
) {
    unsafe {
        let (param_id, value) = text_event(plugin, params, name, text);
        let mut event = clap_event_param_value {
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
        };
        let events = clap_input_events {
            ctx: (&mut event as *mut clap_event_param_value).cast(),
            size: Some(event_count),
            get: Some(event_get),
        };
        let output = clap_output_events {
            ctx: ptr::null_mut(),
            try_push: Some(output_push),
        };
        (*params).flush.unwrap()(plugin, &events, &output);
    }
}

/// A multi-sample block proves note timing is honored within a host buffer.
unsafe fn process_block(
    plugin: *const clap_plugin,
    event: Option<&clap_event_header>,
) -> [f32; 64] {
    unsafe {
        let mut audio = [[0.1_f32; 64]; 2];
        audio[1].fill(-0.1);
        let mut sidechain = [[0.0_f32; 64]; 2];
        let mut output = [[f32::NAN; 64]; 2];
        let mut main_channels = [audio[0].as_mut_ptr(), audio[1].as_mut_ptr()];
        let mut sidechain_channels = [sidechain[0].as_mut_ptr(), sidechain[1].as_mut_ptr()];
        let mut output_channels = [output[0].as_mut_ptr(), output[1].as_mut_ptr()];
        let main = clap_audio_buffer {
            data32: main_channels.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let inputs = [
            main,
            clap_audio_buffer {
                data32: sidechain_channels.as_mut_ptr(),
                ..main
            },
        ];
        let mut outputs = clap_audio_buffer {
            data32: output_channels.as_mut_ptr(),
            ..main
        };
        let events = clap_input_events {
            ctx: event.map_or(ptr::null_mut(), |e| {
                (e as *const clap_event_header).cast_mut().cast()
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
            frames_count: 64,
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
        assert!(output.iter().flatten().all(|v| v.is_finite()));
        output[0]
    }
}

pub(super) unsafe fn verify(plugin: *const clap_plugin, params: *const clap_plugin_params) {
    unsafe {
        let ports = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_NOTE_PORTS.as_ptr())
            .cast::<clap_plugin_note_ports>();
        assert!(!ports.is_null());
        assert_eq!((*ports).count.unwrap()(plugin, true), 1);
        let mut info = std::mem::zeroed::<clap_note_port_info>();
        assert!((*ports).get.unwrap()(plugin, 0, true, &mut info));
        assert_ne!(info.supported_dialects & CLAP_NOTE_DIALECT_MIDI, 0);
        assert_ne!(info.supported_dialects & CLAP_NOTE_DIALECT_CLAP, 0);

        for (name, text) in [
            ("Pitch Source", "MIDI"),
            ("MIDI Bend Range", "12 st"),
            ("Lookahead", "Off"),
            ("Operating Mode", "Track EQ"),
            ("Bypass", "Off"),
            ("F0 type", "Peak"),
            ("F0 gain", "-6 dB"),
            ("F0 ratio", "1"),
            ("F0 enabled", "On"),
            ("Band 2 exists", "Off"),
            ("Band 1 dynamic enabled", "Off"),
            ("Infinite Release", "Off"),
            ("Tracking", "5 ms"),
            ("Attack", "0.1 ms"),
            ("Release", "1 ms"),
        ] {
            set_param(plugin, params, name, text);
        }

        let state = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_STATE.as_ptr())
            .cast::<clap_plugin_state>();
        let mut saved = Vec::<u8>::new();
        let writer = clap_ostream {
            ctx: (&mut saved as *mut Vec<u8>).cast(),
            write: Some(write_state),
        };
        assert!((*state).save.unwrap()(plugin, &writer));
        set_param(plugin, params, "Pitch Source", "Audio");
        set_param(plugin, params, "MIDI Bend Range", "2 st");
        let mut saved = Cursor::new(saved);
        let reader = clap_istream {
            ctx: (&mut saved as *mut Cursor<Vec<u8>>).cast(),
            read: Some(read_state),
        };
        assert!((*state).load.unwrap()(plugin, &reader));
        for (name, text) in [("Pitch Source", "MIDI"), ("MIDI Bend Range", "12 st")] {
            let (id, expected) = text_event(plugin, params, name, text);
            let mut value = 0.0;
            assert!((*params).get_value.unwrap()(plugin, id, &mut value));
            assert_eq!(value, expected);
        }

        assert!((*plugin).activate.unwrap()(plugin, 48000.0, 1, 512));
        let latency = (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_LATENCY.as_ptr())
            .cast::<clap_plugin_latency>();
        assert_eq!((*latency).get.unwrap()(plugin), 0);
        assert!((*plugin).start_processing.unwrap()(plugin));
        (*plugin).reset.unwrap()(plugin);

        let mut note = clap_event_note {
            header: clap_event_header {
                size: size_of::<clap_event_note>() as u32,
                time: 17,
                space_id: 0,
                type_: CLAP_EVENT_NOTE_ON,
                flags: 0,
            },
            note_id: 17,
            port_index: 0,
            channel: 0,
            key: 57,
            velocity: 1.0,
        };
        let output = process_block(plugin, Some(&note.header));
        assert_eq!(&output[..17], &[0.1; 17]);
        assert!(output[17..].iter().any(|v| (*v - 0.1).abs() > 1e-6));
        let gain = render(plugin, None);
        assert!(
            (gain + 6.0).abs() < 0.1,
            "MIDI note did not tune EQ: {gain} dB"
        );

        let bend = clap_event_midi {
            header: clap_event_header {
                size: size_of::<clap_event_midi>() as u32,
                time: 0,
                space_id: 0,
                type_: CLAP_EVENT_MIDI,
                flags: 0,
            },
            port_index: 0,
            data: [0xe0, 127, 127],
        };
        lookahead_tests::process_event(plugin, [0.0; 2], Some(&bend.header));
        let gain = render(plugin, None);
        assert!(gain > -0.5, "MIDI octave bend did not move EQ: {gain} dB");

        note.header.time = 0;
        note.header.type_ = CLAP_EVENT_NOTE_OFF;
        lookahead_tests::process_event(plugin, [0.0; 2], Some(&note.header));
        let gain = render(plugin, None);
        assert!(gain.abs() < 1e-6, "Note off did not release EQ: {gain} dB");
        (*plugin).stop_processing.unwrap()(plugin);
        (*plugin).deactivate.unwrap()(plugin);
        println!("MIDI ports / state restore / sample timing / EQ / pitch bend / note off passed");
    }
}
