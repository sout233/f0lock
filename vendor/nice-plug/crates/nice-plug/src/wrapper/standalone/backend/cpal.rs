use anyhow::{Context, Result};
use cpal::{
    Device, FromSample, I24, InputCallbackInfo, OutputCallbackInfo, Sample, SampleFormat, Stream,
    StreamConfig, traits::*,
};
use crossbeam::sync::{Parker, Unparker};
use midir::{
    MidiInput, MidiInputConnection, MidiInputPort, MidiOutput, MidiOutputConnection, MidiOutputPort,
};
use nice_plug_core::audio_setup::{AudioIOLayout, AuxiliaryBuffers};
use nice_plug_core::buffer::Buffer;
use nice_plug_core::context::process::Transport;
use nice_plug_core::midi::{MidiConfig, NoteEvent, PluginNoteEvent};
use nice_plug_core::plugin::Plugin;
use parking_lot::Mutex;
use rtrb::RingBuffer;
use std::borrow::Borrow;
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::thread::ScopedJoinHandle;

use super::super::config::WrapperConfig;
use super::Backend;
use crate::midi::MidiResult;
use crate::wrapper::util::buffer_management::{BufferManager, ChannelPointers};

/// Uses CPAL for audio and midir for MIDI.
pub struct CpalMidir {
    config: WrapperConfig,
    audio_io_layout: AudioIOLayout,

    input: Option<CpalDevice>,
    output: CpalDevice,

    midi_input: Mutex<Option<MidirInputDevice>>,
    midi_output: Mutex<Option<MidirOutputDevice>>,
}

/// All data needed for a CPAL input or output stream.
struct CpalDevice {
    pub device: Device,
    pub config: StreamConfig,
    pub sample_format: SampleFormat,
}

/// All data needed to create a Midir input stream.
struct MidirInputDevice {
    pub backend: MidiInput,
    pub port: MidiInputPort,
}

/// An active `MidirInputDevice`. Transformed back and from this during the `.run()` function.
struct ActiveMidirInputDevice {
    pub connection: MidiInputConnection<()>,
    pub port: MidiInputPort,
}

/// All data needed to create a Midir output stream.
struct MidirOutputDevice {
    pub backend: MidiOutput,
    pub port: MidiOutputPort,
}

/// An active `MidirOutputDevice`. Transformed back and from this during the `.run()` function.
struct ActiveMidirOutputDevice {
    pub connection: MidiOutputConnection,
    pub port: MidiOutputPort,
}

/// Send+Sync wrapper for `Vec<*mut f32>` so we can preallocate channel pointer vectors for use with
/// the `BufferManager` API.
struct ChannelPointerVec(Vec<*mut f32>);

unsafe impl Send for ChannelPointerVec {}
unsafe impl Sync for ChannelPointerVec {}

impl ChannelPointerVec {
    // If you directly access the `.0` field then it will try to move it out of the struct which
    // undoes the Send+Sync impl.
    pub fn get(&mut self) -> &mut Vec<*mut f32> {
        &mut self.0
    }
}

/// A task for the MIDI output thread.
enum MidiOutputTask<P: Plugin> {
    /// Send an event as MIDI data.
    Send(PluginNoteEvent<P>),
    /// Terminate the thread, stopping it from blocking and allowing it to be joined.
    Terminate,
}

impl<P: Plugin> Backend<P> for CpalMidir {
    fn run(
        &mut self,
        cb: impl FnMut(
            &mut Buffer,
            &mut AuxiliaryBuffers,
            Transport,
            &[PluginNoteEvent<P>],
            &mut Vec<PluginNoteEvent<P>>,
        ) -> bool
        + 'static
        + Send,
    ) {
        // So this is a lot of fun. There are up to four separate streams here, all using their own
        // callbacks. The audio output stream acts as the primary stream, and everything else either
        // sends data to it or (in the case of the MIDI output stream) receives data from it using
        // channels.
        //
        // Audio input is read from the input device (if configured), and is send to the output stream
        // via the `fixed-resample` crate. CPAL audio devices may also not accept floating point samples,
        // so all of the actual audio handling and buffer management handles in the
        // `build_*_data_callback()` functions defined below.
        //
        // MIDI input is parsed in the Midir callback and the events are sent over a callback to the
        // output audio thread where the process callback happens. If that process callback outputs
        // events then those are sent over another ringbuffer to a thread that handles MIDI output.
        // Both MIDI input and MIDI output are disabled by default.
        //
        // The thread scope is needed to accomodate the midir MIDI output API. Outputting MIDI is
        // realtime unsafe, and to be able to output MIDI with midir you need to transform between
        // `MidiOutputPort` and `MidiOutputPortConnection` types by taking values out of an
        // `Option`.
        std::thread::scope(|s| {
            let mut _input_stream: Option<Stream> = None;
            let mut input_rb_consumer: Option<fixed_resample::ResamplingCons<f32>> = None;
            if let Some(input) = &self.input {
                // Data is sent to the output data callback using the fixed-resample crate.
                let (rb_producer, rb_consumer) = fixed_resample::resampling_channel(
                    input.config.channels as usize,
                    input.config.sample_rate,
                    input.config.sample_rate,
                    true,
                    fixed_resample::ResamplingChannelConfig {
                        latency_seconds: self.config.input_latency as f64,
                        capacity_seconds: self.config.input_capacity as f64,
                        ..Default::default()
                    },
                );

                input_rb_consumer = Some(rb_consumer);

                let input_parker = Parker::new();
                let input_unparker = input_parker.unparker().clone();
                let error_cb = {
                    let input_unparker = input_unparker.clone();
                    move |err: cpal::Error| {
                        crate::nice_error!("Error during capture: {err:#}");

                        if !matches!(
                            err.kind(),
                            cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::Xrun
                        ) {
                            input_unparker.clone().unpark();
                        }
                    }
                };

                let conversion_buf: Vec<f32> =
                    vec![0.0; self.config.period_size as usize * input.config.channels as usize];

                crate::nice_log!(
                    "Starting input stream: {:?} {:?}",
                    &input.device,
                    &input.config
                );

                macro_rules! build_input_streams {
                    ($sample_format:expr, $(($format:path, $primitive_type:ty)),*) => {
                        match $sample_format {
                            $($format => input.device.build_input_stream(
                                input.config,
                                self.build_input_data_callback::<$primitive_type>(input_unparker, conversion_buf, rb_producer),
                                error_cb,
                                None,
                            ),)*
                            format => todo!("Unsupported sample format {format}"),
                        }
                    }
                }
                let stream = build_input_streams!(
                    input.sample_format,
                    (SampleFormat::I8, i8),
                    (SampleFormat::I16, i16),
                    (SampleFormat::I24, I24),
                    (SampleFormat::I32, i32),
                    (SampleFormat::I64, i64),
                    (SampleFormat::U8, u8),
                    (SampleFormat::U16, u16),
                    (SampleFormat::U32, u32),
                    (SampleFormat::U64, u64),
                    (SampleFormat::F32, f32),
                    (SampleFormat::F64, f64)
                )
                .expect("Fatal error creating the capture stream");

                if let Err(e) = stream.play() {
                    if let cpal::ErrorKind::RealtimeDenied = e.kind() {
                        crate::nice_warn!(
                            "Failed to set realtime priority for input audio stream. This may \
                             result in increased latency and/or audio glitches."
                        );
                    } else {
                        panic!("Fatal error playing the output stream: {}", e);
                    }
                }

                _input_stream = Some(stream);

                // Playback is delayed one period if we're capturing audio so it has something to
                // process
                input_parker.park()
            }

            // The output callback can read input events from this ringbuffer
            let mut midi_input_rb_consumer: Option<rtrb::Consumer<PluginNoteEvent<P>>> = None;
            let midi_input_connection: Option<ActiveMidirInputDevice> =
                self.midi_input.lock().take().and_then(|midi_input| {
                    // Data is sent to the output data callback using a wait-free ring buffer
                    let (rb_producer, rb_consumer) =
                        RingBuffer::new(self.config.midi_capacity as usize);
                    midi_input_rb_consumer = Some(rb_consumer);

                    let result = midi_input.backend.connect(
                        &midi_input.port,
                        "MIDI input",
                        self.build_midi_input_thread::<P>(rb_producer),
                        (),
                    );

                    match result {
                        Ok(connection) => Some(ActiveMidirInputDevice {
                            connection,
                            port: midi_input.port,
                        }),
                        Err(err) => {
                            // We won't retry once this fails
                            crate::nice_error!(
                                "Could not create the MIDI input connection: {err:#}"
                            );
                            midi_input_rb_consumer = None;

                            None
                        }
                    }
                });

            // The output callback can also emit MIDI events. To handle these we'll need to spawn
            // our own thread. This can be simplified a lot by using the `MidiOutputConnection`
            // directly inside the audio output callback, but looking at the implementation sending
            // MIDI events is not realtime safe in most midir backends.
            // NOTE: This uses crossbeam channels instead of rtrb specifically for the optional
            //        blocking API. This lets the MIDI sending thread sleep when there's no work to
            //        do.
            let mut midi_output_rb_producer: Option<crossbeam::channel::Sender<MidiOutputTask<P>>> =
                None;
            let midi_output_connection: Option<ScopedJoinHandle<ActiveMidirOutputDevice>> =
                self.midi_output.lock().take().and_then(|midi_output| {
                    // This uses crossbeam channels for the reason mentioned above, but to keep
                    // things cohesive we'll use the same naming scheme as we use for rtrb
                    let (sender, receiver) =
                        crossbeam::channel::bounded(self.config.midi_capacity as usize);
                    midi_output_rb_producer = Some(sender);

                    let result = midi_output
                        .backend
                        .connect(&midi_output.port, "MIDI output");

                    match result {
                        Ok(mut connection) => Some(s.spawn(move || {
                            while let Ok(task) = receiver.recv() {
                                match task {
                                    MidiOutputTask::Send(event) => match event.as_midi() {
                                        Some(MidiResult::Basic(midi_data)) => {
                                            if let Err(err) = connection.send(&midi_data) {
                                                crate::nice_error!(
                                                    "Could not send MIDI event: {err}"
                                                );
                                            }
                                        }
                                        Some(MidiResult::SysEx(padded_sysex_buffer, length)) => {
                                            // The SysEx buffer may contain padding
                                            let padded_sysex_buffer = padded_sysex_buffer.borrow();
                                            crate::nice_debug_assert!(
                                                length <= padded_sysex_buffer.len()
                                            );

                                            if let Err(err) =
                                                connection.send(&padded_sysex_buffer[..length])
                                            {
                                                crate::nice_error!(
                                                    "Could not send MIDI event: {err}"
                                                );
                                            }
                                        }
                                        None => (),
                                    },
                                    MidiOutputTask::Terminate => break,
                                }
                            }

                            // We'll return the same value from the join handle as what ends up
                            // being stored in `midi_input_connection` to keep this symmetrical with
                            // the input handling
                            ActiveMidirOutputDevice {
                                connection,
                                port: midi_output.port,
                            }
                        })),
                        Err(err) => {
                            crate::nice_error!(
                                "Could not create the MIDI output connection: {err:#}"
                            );
                            midi_output_rb_producer = None;

                            None
                        }
                    }
                });

            // This thread needs to be blocked until audio processing ends as CPAL processes the
            // streams on another thread instead of blocking
            let parker = Parker::new();
            let unparker = parker.unparker().clone();
            let error_cb = {
                let unparker = unparker.clone();
                move |err: cpal::Error| {
                    crate::nice_error!("Error during playback: {err:#}");

                    if !matches!(
                        err.kind(),
                        cpal::ErrorKind::DeviceChanged | cpal::ErrorKind::Xrun
                    ) {
                        unparker.clone().unpark();
                    }
                }
            };

            crate::nice_log!(
                "Starting output stream: {:?} {:?}",
                &self.output.device,
                &self.output.config
            );

            macro_rules! build_output_streams {
                ($sample_format:expr, $(($format:path, $primitive_type:ty)),*) => {
                    match $sample_format {
                        $($format => self.output.device.build_output_stream(
                            self.output.config,
                            self.build_output_data_callback::<P, $primitive_type>(
                                unparker,
                                input_rb_consumer,
                                midi_input_rb_consumer,
                                // This is a MPMC crossbeam channel instead of an rtrb ringbuffer, and we
                                // also need it to terminate the thread
                                midi_output_rb_producer.clone(),
                                cb,
                            ),
                            error_cb,
                            None,
                        ),)*
                        format => todo!("Unsupported sample format {format}"),
                    }
                }
            }
            let output_stream = build_output_streams!(
                self.output.sample_format,
                (SampleFormat::I8, i8),
                (SampleFormat::I16, i16),
                (SampleFormat::I24, I24),
                (SampleFormat::I32, i32),
                (SampleFormat::I64, i64),
                (SampleFormat::U8, u8),
                (SampleFormat::U16, u16),
                (SampleFormat::U32, u32),
                (SampleFormat::U64, u64),
                (SampleFormat::F32, f32),
                (SampleFormat::F64, f64)
            )
            .expect("Fatal error creating the output stream");

            if let Err(e) = output_stream.play() {
                if let cpal::ErrorKind::RealtimeDenied = e.kind() {
                    crate::nice_warn!(
                        "Failed to set realtime priority for output audio stream. This may result \
                         in increased latency and/or audio glitches."
                    );
                } else {
                    panic!("Fatal error playing the output stream: {}", e);
                }
            }

            // Wait for the audio thread to exit
            parker.park();

            // The Midir API requires us to take things out of Options and transform between these
            // structs
            *self.midi_input.lock() =
                midi_input_connection.map(|midi_input_connection| MidirInputDevice {
                    backend: midi_input_connection.connection.close().0,
                    port: midi_input_connection.port,
                });
            *self.midi_output.lock() =
                midi_output_connection.map(move |midi_output_connection_handle| {
                    // The thread needs to be terminated first
                    midi_output_rb_producer
                        .expect("Inconsistent internal MIDI output state")
                        .send(MidiOutputTask::Terminate)
                        .expect("Could not terminate the MIDI output thread");

                    let midi_output_connection = midi_output_connection_handle
                        .join()
                        .expect("MIDI output thread panicked");

                    MidirOutputDevice {
                        backend: midi_output_connection.connection.close(),
                        port: midi_output_connection.port,
                    }
                });
        });
    }
}

impl CpalMidir {
    /// Initialize the backend with the specified host. Returns an error if this failed for whatever
    /// reason.
    pub fn new<P: Plugin>(config: WrapperConfig, cpal_host_id: cpal::HostId) -> Result<Self> {
        let audio_io_layout = config.audio_io_layout_or_exit::<P>();

        // ASIO exposes input and output through one duplex device, so reuse the selected input
        // device when no output was specified.
        #[cfg(all(target_os = "windows", feature = "standalone-asio"))]
        let output_device_name = if cpal_host_id == cpal::HostId::Asio {
            config
                .output_device
                .as_ref()
                .or(config.input_device.as_ref())
        } else {
            config.output_device.as_ref()
        };
        #[cfg(not(all(target_os = "windows", feature = "standalone-asio")))]
        let output_device_name = config.output_device.as_ref();

        let host = cpal::host_from_id(cpal_host_id).context("The Audio API is unavailable")?;

        if config.input_device.is_none() && audio_io_layout.main_input_channels.is_some() {
            crate::nice_log!(
                "Audio inputs are not connected automatically to prevent feedback. Use the \
                 '--input-device' option to choose an input device."
            )
        }

        if config.midi_input.is_none() && P::MIDI_INPUT >= MidiConfig::Basic {
            crate::nice_log!("Use the '--midi-input' option to select a MIDI input device.")
        }
        if config.midi_output.is_none() && P::MIDI_OUTPUT >= MidiConfig::Basic {
            crate::nice_log!("Use the '--midi-output' option to select a MIDI output device.")
        }

        // No input device is connected unless requested by the user to avoid feedback loops
        let input_device = config
            .input_device
            .as_ref()
            .map(|name| -> Result<Device> {
                let device = host
                    .input_devices()
                    .context("No audio input devices available")?
                    // `.name()` returns a `Result` with a non-Eq error type so you can't compare this
                    // directly
                    .find(|d| d.description().map(|n| n.name() == name).unwrap_or(false))
                    .with_context(|| {
                        // This is a bit awkward, but instead of adding a dedicated option we'll just
                        // list all of the available devices in the error message when the chosen device
                        // does not exist
                        let mut message =
                            format!("Unknown input device '{name}'. Available devices are:");
                        for device_name in host
                            .input_devices()
                            .unwrap()
                            .filter_map(|d| d.description().map(|d| d.name().to_string()).ok())
                        {
                            message.push_str(&format!("\n{device_name}"))
                        }

                        message
                    })?;

                Ok(device)
            })
            .transpose()?;

        let output_device = match output_device_name {
            Some(name) => host
                .output_devices()
                .context("No audio output devices available")?
                .find(|d| d.description().map(|n| n.name() == name).unwrap_or(false))
                .with_context(|| {
                    let mut message =
                        format!("Unknown output device '{name}'. Available devices are:");
                    for device_name in host
                        .output_devices()
                        .unwrap()
                        .filter_map(|d| d.description().map(|d| d.name().to_string()).ok())
                    {
                        message.push_str(&format!("\n{device_name}"))
                    }

                    message
                })?,
            None => host
                .default_output_device()
                .context("No default audio output device available")?,
        };

        let requested_sample_rate = config.sample_rate as u32;
        let requested_buffer_size = cpal::BufferSize::Fixed(config.period_size);
        let num_input_channels = audio_io_layout
            .main_input_channels
            .map(NonZeroU32::get)
            .unwrap_or_default() as usize;
        let input = input_device
            .map(|device| -> Result<CpalDevice> {
                let input_configs: Vec<_> = device
                    .supported_input_configs()
                    .context("Could not get supported audio input configurations")?
                    .filter(|c| match c.buffer_size() {
                        cpal::SupportedBufferSize::Range { min, max } => {
                            c.channels() as usize == num_input_channels
                                && (c.min_sample_rate()..=c.max_sample_rate())
                                    .contains(&requested_sample_rate)
                                && (min..=max).contains(&&config.period_size)
                        }
                        cpal::SupportedBufferSize::Unknown => false,
                    })
                    .collect();
                let input_config_range = input_configs
                    .iter()
                    // Prefer floating point samples to avoid conversions
                    .find(|c| c.sample_format() == SampleFormat::F32)
                    .or_else(|| input_configs.first())
                    .cloned()
                    .with_context(|| {
                        format!(
                            "The audio input device does not support {} audio channels at a \
                             sample rate of {} Hz and a period size of {} samples",
                            num_input_channels, config.sample_rate, config.period_size,
                        )
                    })?;

                // We already checked that these settings are valid
                let input_config = StreamConfig {
                    channels: input_config_range.channels(),
                    sample_rate: requested_sample_rate,
                    buffer_size: requested_buffer_size,
                };
                let input_sample_format = input_config_range.sample_format();

                Ok(CpalDevice {
                    device,
                    config: input_config,
                    sample_format: input_sample_format,
                })
            })
            .transpose()?;

        let num_output_channels = audio_io_layout
            .main_output_channels
            .map(NonZeroU32::get)
            .unwrap_or_default() as usize;
        let output = {
            let output_configs: Vec<_> = output_device
                .supported_output_configs()
                .context("Could not get supported audio output configurations")?
                .filter(|c| match c.buffer_size() {
                    cpal::SupportedBufferSize::Range { min, max } => {
                        (c.min_sample_rate()..=c.max_sample_rate()).contains(&requested_sample_rate)
                            && (min..=max).contains(&&config.period_size)
                    }
                    cpal::SupportedBufferSize::Unknown => false,
                })
                .collect();
            // Prefer a config with at least as many channels as the plugin, otherwise fall back to
            // the one with the most channels so we can still play a plugin with more outputs than
            // the device has (e.g. an ambisonic plugin on a stereo device).
            let channel_distance = |c: &cpal::SupportedStreamConfigRange| {
                let channels = c.channels() as usize;
                if channels >= num_output_channels {
                    (0u8, channels - num_output_channels)
                } else {
                    (1u8, num_output_channels - channels)
                }
            };
            let output_config_range = output_configs
                .iter()
                .filter(|c| c.sample_format() == SampleFormat::F32)
                .min_by_key(|c| channel_distance(c))
                .or_else(|| output_configs.iter().min_by_key(|c| channel_distance(c)))
                .cloned()
                .with_context(|| {
                    format!(
                        "The audio output device does not support any channel layout at a sample \
                         rate of {} Hz and a period size of {} samples",
                        config.sample_rate, config.period_size,
                    )
                })?;
            let output_config = StreamConfig {
                channels: output_config_range.channels(),
                sample_rate: requested_sample_rate,
                buffer_size: requested_buffer_size,
            };
            let output_sample_format = output_config_range.sample_format();

            CpalDevice {
                device: output_device,
                config: output_config,
                sample_format: output_sample_format,
            }
        };

        let device_output_channels = output.config.channels as usize;
        if device_output_channels < num_output_channels {
            crate::nice_warn!(
                "The audio output device only has {} channels, but the plugin has {} output \
                 channels. Only the first {} channels will be played back.",
                device_output_channels,
                num_output_channels,
                device_output_channels,
            );
        }

        // There's no obvious way to do sidechain inputs and additional outputs with the CPAL
        // backends like there is with JACK. So we'll just provide empty buffers instead.
        if !audio_io_layout.aux_input_ports.is_empty() {
            crate::nice_warn!("Sidechain inputs are not supported with this audio backend");
        }
        if !audio_io_layout.aux_output_ports.is_empty() {
            crate::nice_warn!("Auxiliary outputs are not supported with this audio backend");
        }

        let midi_input = match &config.midi_input {
            Some(midi_input_name) => {
                // Midir lets us preemptively ignore MIDI messages we'll never use like active
                // sensing and timing, but for maximum flexibility with nice-plug's SysEx parsing
                // types (which could technically be used to also parse those things) we won't do
                // that.
                let midi_backend = MidiInput::new(P::NAME)
                    .context("Could not initialize the MIDI input backend")?;
                let available_ports = midi_backend.ports();

                // In case there somehow is a MIDI port with an empty name, we'll still want to
                // preserve the behavior of an empty argument resulting in a listing of options.
                let found_port = if !midi_input_name.is_empty() {
                    // This API is a bit weird
                    available_ports
                        .iter()
                        .find(|port| midi_backend.port_name(port).as_deref() == Ok(midi_input_name))
                } else {
                    None
                };

                match found_port {
                    Some(port) => Some(MidirInputDevice {
                        backend: midi_backend,
                        port: port.clone(),
                    }),
                    None => {
                        let mut message = format!(
                            "Unknown input MIDI device '{midi_input_name}'. Available devices are:"
                        );
                        for port in available_ports {
                            match midi_backend.port_name(&port) {
                                Ok(device_name) => message.push_str(&format!("\n{device_name}")),
                                Err(err) => message.push_str(&format!("\nERROR: {err:#}")),
                            }
                        }

                        anyhow::bail!(message);
                    }
                }
            }
            None => None,
        };

        let midi_output = match &config.midi_output {
            Some(midi_output_name) => {
                let midi_backend = MidiOutput::new(P::NAME)
                    .context("Could not initialize the MIDI output backend")?;
                let available_ports = midi_backend.ports();

                let found_port = if !midi_output_name.is_empty() {
                    available_ports.iter().find(|port| {
                        midi_backend.port_name(port).as_deref() == Ok(midi_output_name)
                    })
                } else {
                    None
                };

                match found_port {
                    Some(port) => Some(MidirOutputDevice {
                        backend: midi_backend,
                        port: port.clone(),
                    }),
                    None => {
                        let mut message = format!(
                            "Unknown output MIDI device '{midi_output_name}'. Available devices \
                             are:"
                        );
                        for port in available_ports {
                            match midi_backend.port_name(&port) {
                                Ok(device_name) => message.push_str(&format!("\n{device_name}")),
                                Err(err) => message.push_str(&format!("\nERROR: {err:#}")),
                            }
                        }

                        anyhow::bail!(message);
                    }
                }
            }
            None => None,
        };

        Ok(CpalMidir {
            config,
            audio_io_layout,

            input,
            output,

            midi_input: Mutex::new(midi_input),
            midi_output: Mutex::new(midi_output),
        })
    }

    fn build_input_data_callback<T>(
        &self,
        input_unparker: Unparker,
        mut conversion_buf: Vec<f32>,
        mut input_rb_producer: fixed_resample::ResamplingProd<f32>,
    ) -> impl FnMut(&[T], &InputCallbackInfo) + Send + 'static
    where
        T: Sample,
        // The CPAL update made the whole interface more complicated by switching to dasp's sample
        // trait, and then they also forgot to expose the `ToSample` trait so now you need to do
        // this
        f32: FromSample<T>,
    {
        // This callback needs to copy input samples to a ring buffer that can be read from in the
        // output data callback
        move |data, _info| {
            for data_chunk in data.chunks(conversion_buf.len()) {
                for (out_s, &in_s) in conversion_buf.iter_mut().zip(data_chunk.iter()) {
                    *out_s = FromSample::from_sample_(in_s);
                }

                let _ = input_rb_producer.push_interleaved(&conversion_buf[..data_chunk.len()]);
            }

            // The run function is blocked until a single period has been processed here. After this
            // point output playback can start.
            input_unparker.unpark();
        }
    }

    fn build_midi_input_thread<P: Plugin>(
        &self,
        mut midi_input_rb_producer: rtrb::Producer<PluginNoteEvent<P>>,
    ) -> impl FnMut(u64, &[u8], &mut ()) + Send + 'static {
        // This callback parses the received MIDI bytes and sends them to a ring buffer
        move |_timing, midi_data, _data| {
            // Since this is system MIDI there's no real useful timing information and we'll set all
            // the timings to the first sample in the buffer
            if let Ok(event) = NoteEvent::from_midi(0, midi_data)
                && midi_input_rb_producer.push(event).is_err()
            {
                crate::nice_error!("The MIDI input event queue was full, dropping event");
            }
        }
    }

    fn build_output_data_callback<P, T>(
        &self,
        unparker: Unparker,
        mut input_rb_consumer: Option<fixed_resample::ResamplingCons<f32>>,
        mut input_event_rb_consumer: Option<rtrb::Consumer<PluginNoteEvent<P>>>,
        mut output_event_rb_producer: Option<crossbeam::channel::Sender<MidiOutputTask<P>>>,
        mut cb: impl FnMut(
            &mut Buffer,
            &mut AuxiliaryBuffers,
            Transport,
            &[PluginNoteEvent<P>],
            &mut Vec<PluginNoteEvent<P>>,
        ) -> bool
        + 'static
        + Send,
    ) -> impl FnMut(&mut [T], &OutputCallbackInfo) + Send + 'static
    where
        P: Plugin,
        T: Sample + FromSample<f32> + Default,
    {
        // We'll receive interlaced input samples from CPAL. These need to converted to deinterlaced
        // channels, processed, and then copied those back to an interlaced buffer for the output.
        let buffer_size = self.config.period_size as usize;
        let device_output_channels = self.output.config.channels as usize;
        let num_output_channels = self
            .audio_io_layout
            .main_output_channels
            .map(NonZeroU32::get)
            .unwrap_or(0) as usize;
        let num_input_channels = self
            .audio_io_layout
            .main_input_channels
            .map(NonZeroU32::get)
            .unwrap_or(0) as usize;
        // This may contain excess unused space at the end if we get fewer samples than configured
        // from CPAL
        let mut main_io_storage = vec![vec![0.0f32; buffer_size]; num_output_channels];

        // This backend does not support auxiliary inputs and outputs, so in order to have the same
        // behavior as the other backends we'll provide some dummy buffers that we'll zero out every
        // time
        let mut aux_input_storage: Vec<Vec<Vec<f32>>> = Vec::new();
        for channel_count in self.audio_io_layout.aux_input_ports {
            aux_input_storage.push(vec![
                vec![0.0f32; buffer_size];
                channel_count.get() as usize
            ]);
        }

        let mut aux_output_storage: Vec<Vec<Vec<f32>>> = Vec::new();
        for channel_count in self.audio_io_layout.aux_output_ports {
            aux_output_storage.push(vec![
                vec![0.0f32; buffer_size];
                channel_count.get() as usize
            ]);
        }

        // The actual buffer management here works the same as in the JACK backend. See that
        // implementation for more information.
        let mut buffer_manager =
            BufferManager::for_audio_io_layout(buffer_size, self.audio_io_layout);
        let mut main_io_channel_pointers = ChannelPointerVec(Vec::with_capacity(
            self.audio_io_layout
                .main_output_channels
                .map(NonZeroU32::get)
                .unwrap_or(0) as usize,
        ));
        let mut aux_input_channel_pointers =
            Vec::with_capacity(self.audio_io_layout.aux_input_ports.len());
        for channel_count in self.audio_io_layout.aux_input_ports {
            aux_input_channel_pointers.push(ChannelPointerVec(Vec::with_capacity(
                channel_count.get() as usize,
            )));
        }
        let mut aux_output_channel_pointers =
            Vec::with_capacity(self.audio_io_layout.aux_output_ports.len());
        for channel_count in self.audio_io_layout.aux_output_ports {
            aux_output_channel_pointers.push(ChannelPointerVec(Vec::with_capacity(
                channel_count.get() as usize,
            )));
        }

        let mut midi_input_events = Vec::with_capacity(self.config.midi_capacity as usize);
        let mut midi_output_events = Vec::with_capacity(self.config.midi_capacity as usize);

        // Can't borrow from `self` in the callback
        let config = self.config.clone();
        let mut num_processed_samples = 0usize;
        let mut finished = false;
        move |data, _info| {
            if !finished {
                'chunk: for data in data.chunks_mut(buffer_size * device_output_channels) {
                    let out_frames = data.len() / device_output_channels;

                    let mut transport = Transport::new(config.sample_rate);
                    transport.pos_samples = Some(num_processed_samples as i64);
                    transport.tempo = Some(config.tempo as f64);
                    transport.time_sig_numerator = Some(config.timesig_num as i32);
                    transport.time_sig_denominator = Some(config.timesig_denom as i32);
                    transport.playing = true;

                    // If an input was configured, deinterleave input samples from the RB into
                    // main_io_storage. Excess output channels are zeroed.
                    match &mut input_rb_consumer {
                        Some(input_rb_consumer) => {
                            let in_channels =
                                input_rb_consumer.num_channels().min(main_io_storage.len());
                            let _ = input_rb_consumer.read(
                                &mut audioadapter_buffers::direct::SequentialSliceOfVecs::new_mut(
                                    &mut main_io_storage,
                                    in_channels,
                                    out_frames,
                                )
                                .unwrap(),
                                None,
                                None,
                                false,
                            );

                            for channel in main_io_storage
                                .iter_mut()
                                .skip(input_rb_consumer.num_channels())
                            {
                                channel.fill(0.0);
                            }
                        }
                        None => {
                            for channel in main_io_storage.iter_mut() {
                                channel.fill(0.0);
                            }
                        }
                    }

                    // Things may have been moved in between callbacks, so these pointers need to be set up
                    // again on each invocation
                    main_io_channel_pointers.get().clear();
                    for channel in main_io_storage.iter_mut() {
                        assert!(channel.len() == buffer_size);

                        main_io_channel_pointers.get().push(channel.as_mut_ptr());
                    }

                    for (input_channel_pointers, input_storage) in aux_input_channel_pointers
                        .iter_mut()
                        .zip(aux_input_storage.iter_mut())
                    {
                        input_channel_pointers.get().clear();
                        for channel in input_storage.iter_mut() {
                            assert!(channel.len() == buffer_size);

                            input_channel_pointers.get().push(channel.as_mut_ptr());
                        }
                    }

                    for (output_channel_pointers, output_storage) in aux_output_channel_pointers
                        .iter_mut()
                        .zip(aux_output_storage.iter_mut())
                    {
                        output_channel_pointers.get().clear();
                        for channel in output_storage.iter_mut() {
                            assert!(channel.len() == buffer_size);

                            output_channel_pointers.get().push(channel.as_mut_ptr());
                        }
                    }

                    {
                        let buffers = unsafe {
                            buffer_manager.create_buffers(0, out_frames, |buffer_sources| {
                                *buffer_sources.main_output_channel_pointers =
                                    Some(ChannelPointers {
                                        ptrs: NonNull::new(
                                            main_io_channel_pointers.get().as_mut_ptr(),
                                        )
                                        .unwrap(),
                                        num_channels: main_io_channel_pointers.get().len(),
                                    });
                                *buffer_sources.main_input_channel_pointers =
                                    Some(ChannelPointers {
                                        ptrs: NonNull::new(
                                            main_io_channel_pointers.get().as_mut_ptr(),
                                        )
                                        .unwrap(),
                                        num_channels: num_input_channels
                                            .min(main_io_channel_pointers.get().len()),
                                    });

                                for (input_source_channel_pointers, input_channel_pointers) in
                                    buffer_sources
                                        .aux_input_channel_pointers
                                        .iter_mut()
                                        .zip(aux_input_channel_pointers.iter_mut())
                                {
                                    *input_source_channel_pointers = Some(ChannelPointers {
                                        ptrs: NonNull::new(
                                            input_channel_pointers.get().as_mut_ptr(),
                                        )
                                        .unwrap(),
                                        num_channels: input_channel_pointers.get().len(),
                                    });
                                }

                                for (output_source_channel_pointers, output_channel_pointers) in
                                    buffer_sources
                                        .aux_output_channel_pointers
                                        .iter_mut()
                                        .zip(aux_output_channel_pointers.iter_mut())
                                {
                                    *output_source_channel_pointers = Some(ChannelPointers {
                                        ptrs: NonNull::new(
                                            output_channel_pointers.get().as_mut_ptr(),
                                        )
                                        .unwrap(),
                                        num_channels: output_channel_pointers.get().len(),
                                    });
                                }
                            })
                        };

                        midi_input_events.clear();
                        if let Some(input_event_rb_consumer) = &mut input_event_rb_consumer {
                            // Drain the whole ring: the midir input callback can
                            // enqueue many events (e.g. a fast-moving CC wheel)
                            // between audio callbacks.
                            while let Ok(event) = input_event_rb_consumer.pop() {
                                midi_input_events.push(event);
                            }
                        }

                        midi_output_events.clear();
                        let mut aux = AuxiliaryBuffers {
                            inputs: buffers.aux_inputs,
                            outputs: buffers.aux_outputs,
                        };
                        if !cb(
                            buffers.main_buffer,
                            &mut aux,
                            transport,
                            &midi_input_events,
                            &mut midi_output_events,
                        ) {
                            finished = true;
                            unparker.unpark();
                            break 'chunk;
                        }
                    }

                    // The buffer's samples need to be written to `data` in an interlaced format
                    // SAFETY: Dropping `buffers` allows us to borrow `main_io_storage` again.
                    if device_output_channels > main_io_storage.len() {
                        // If the output device has more channels than the plugin, make sure that
                        // the extra channels are filled with zeros.
                        data.fill(T::default());
                    }
                    // Drop any plugin channels that don't fit on the device.
                    let copied_channels = num_output_channels.min(device_output_channels);
                    for (ch, in_ch) in main_io_storage.iter().enumerate().take(copied_channels) {
                        for (out_s, &in_s) in data[ch..]
                            .chunks_mut(device_output_channels)
                            .zip(in_ch.iter())
                        {
                            out_s[0] = T::from_sample(in_s);
                        }
                    }

                    if let Some(output_event_rb_producer) = &mut output_event_rb_producer {
                        for event in midi_output_events.drain(..) {
                            if output_event_rb_producer
                                .try_send(MidiOutputTask::Send(event))
                                .is_err()
                            {
                                crate::nice_error!(
                                    "The MIDI output event queue was full, dropping event"
                                );
                                break;
                            }
                        }
                    }

                    num_processed_samples += buffer_size;
                }
            }

            if finished {
                data.fill(T::default());
            }
        }
    }
}
