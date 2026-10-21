use super::*;

pub(super) fn run(plugin_address: usize, stop: Arc<AtomicBool>) -> u64 {
    let plugin = plugin_address as *const clap_plugin;
    assert!(unsafe { (*plugin).start_processing.unwrap()(plugin) });

    let events = clap_input_events {
        ctx: ptr::null_mut(),
        size: Some(event_count),
        get: Some(event_get),
    };
    let output_events = clap_output_events {
        ctx: ptr::null_mut(),
        try_push: Some(output_push),
    };
    let started = Instant::now();
    let mut total = 0_u64;

    while !stop.load(Ordering::Acquire) {
        let mut left = [0.0_f32; 512];
        let mut right = [0.0_f32; 512];
        let mut sc_left = [0.0_f32; 512];
        let mut sc_right = [0.0_f32; 512];
        let mut output_left = [f32::NAN; 512];
        let mut output_right = [f32::NAN; 512];

        for i in 0..512 {
            let seconds = (total + i as u64) as f64 / 48000.0;
            let phase = std::f64::consts::TAU * 220.0 * seconds;
            left[i] = (0.1 * phase.sin() + 0.025 * (phase * 3.0).sin()) as f32;
            right[i] = left[i];
        }

        let mut inputs = [left.as_mut_ptr(), right.as_mut_ptr()];
        let mut sidechain = [sc_left.as_mut_ptr(), sc_right.as_mut_ptr()];
        let mut outputs = [output_left.as_mut_ptr(), output_right.as_mut_ptr()];
        let input = clap_audio_buffer {
            data32: inputs.as_mut_ptr(),
            data64: ptr::null_mut(),
            channel_count: 2,
            latency: 0,
            constant_mask: 0,
        };
        let mut output = clap_audio_buffer {
            data32: outputs.as_mut_ptr(),
            ..input
        };
        let inputs = [
            input,
            clap_audio_buffer {
                data32: sidechain.as_mut_ptr(),
                ..input
            },
        ];
        let process = clap_process {
            steady_time: total as i64,
            frames_count: 512,
            transport: ptr::null(),
            audio_inputs: inputs.as_ptr(),
            audio_outputs: &mut output,
            audio_inputs_count: 2,
            audio_outputs_count: 1,
            in_events: &events,
            out_events: &output_events,
        };

        assert_ne!(
            unsafe { (*plugin).process.unwrap()(plugin, &process) },
            CLAP_PROCESS_ERROR
        );
        assert!(
            output_left
                .iter()
                .chain(&output_right)
                .all(|sample| sample.is_finite())
        );
        total += 512;

        let deadline = started + Duration::from_secs_f64(total as f64 / 48000.0);
        if let Some(delay) = deadline.checked_duration_since(Instant::now()) {
            std::thread::sleep(delay);
        }
    }

    unsafe { (*plugin).stop_processing.unwrap()(plugin) };
    total / 512
}
