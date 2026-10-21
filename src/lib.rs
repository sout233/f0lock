mod analyzer;
mod band;
mod detector;
mod editor;
mod engine;
mod filter;
mod lookahead;
mod mode;
#[cfg(test)]
mod multiband_tests;

#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod lookahead_tests;
mod params;
mod pitch;
mod realtime;
#[cfg(test)]
mod sidechain_tests;
#[cfg(test)]
mod spectrum_tests;
#[cfg(test)]
mod tests;

use engine::{Controls, Engine};
use mode::OperatingMode;
use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier, create_egui_editor,
};
use params::F0Params;
use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

pub(crate) struct Telemetry {
    latency_samples: AtomicU32,
    audio_channels: AtomicU32,
    buffer_samples: AtomicU32,
    values: [AtomicU32; 10],
    spectrum: [AtomicU32; analyzer::BINS],
    bands: [[AtomicU32; band::BAND_VALUES]; band::BAND_COUNT],
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            latency_samples: AtomicU32::new(0),
            audio_channels: AtomicU32::new(0),
            buffer_samples: AtomicU32::new(0),
            values: std::array::from_fn(|_| AtomicU32::new(0)),
            spectrum: std::array::from_fn(|_| AtomicU32::new((-100.0_f32).to_bits())),
            bands: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
        }
    }
}

impl Telemetry {
    fn publish(&self, e: &Engine) {
        self.latency_samples
            .store(e.latency_samples() as u32, Ordering::Relaxed);
        for (destination, source) in self.bands.iter().zip(&e.bands) {
            for (atomic, value) in destination.iter().zip(source.values()) {
                atomic.store(value.to_bits(), Ordering::Relaxed);
            }
        }

        let values = [
            e.estimate.frequency,
            e.estimate.clarity,
            e.frequency,
            e.gain,
            e.q,
            e.input_peak,
            e.output_peak,
            e.sample_rate,
            detector::WINDOW as f32 / e.detector.rate * 1000.0,
            e.estimate.rms,
        ];

        for (atomic, value) in self.values.iter().zip(values) {
            atomic.store(value.to_bits(), Ordering::Relaxed);
        }

        for (atomic, value) in self.spectrum.iter().zip(e.analyzer.display) {
            atomic.store(value.to_bits(), Ordering::Relaxed);
        }
    }

    fn values(&self) -> [f32; 10] {
        std::array::from_fn(|i| f32::from_bits(self.values[i].load(Ordering::Relaxed)))
    }

    fn spectrum(&self) -> [f32; analyzer::BINS] {
        std::array::from_fn(|i| f32::from_bits(self.spectrum[i].load(Ordering::Relaxed)))
    }

    fn bands(&self) -> [[f32; band::BAND_VALUES]; band::BAND_COUNT] {
        std::array::from_fn(|i| {
            std::array::from_fn(|j| f32::from_bits(self.bands[i][j].load(Ordering::Relaxed)))
        })
    }
}

pub struct F0Lock {
    params: Arc<F0Params>,
    telemetry: Arc<Telemetry>,
    editor_state: Arc<EguiEditorState>,
    repaint: RepaintNotifier,
    engine: Engine,
}

impl Default for F0Lock {
    fn default() -> Self {
        Self {
            params: Arc::new(F0Params::default()),
            telemetry: Arc::new(Telemetry::default()),
            editor_state: EguiEditorState::from_size(LogicalSize::new(900.0, 684.0), 1.0),
            repaint: RepaintNotifier::new(),
            engine: Engine::new(48000.0),
        }
    }
}

impl Plugin for F0Lock {
    const NAME: &'static str = "f0lock";
    const VENDOR: &'static str = "f0lock";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");
    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            aux_input_ports: &[new_nonzero_u32(2)],
            names: PortNames {
                aux_inputs: &["Sidechain"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            aux_input_ports: &[new_nonzero_u32(1)],
            names: PortNames {
                aux_inputs: &["Sidechain"],
                ..PortNames::const_default()
            },
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type Editor = EguiEditor<editor::F0Editor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _: AsyncExecutor<Self>) -> Option<Self::Editor> {
        let percent = self.params.ui_scale_percent.load(Ordering::Relaxed);
        let scale = if params::UI_SCALES.contains(&percent) {
            percent as f32 / 100.0
        } else {
            1.0
        };
        self.editor_state.set_size(LogicalSize::new(900.0, 684.0));
        self.editor_state.set_zoom_factor(scale);

        create_egui_editor(
            self.editor_state.clone(),
            self.repaint.clone(),
            editor_settings(),
            editor::F0Editor::new(self.params.clone(), self.telemetry.clone()),
        )
    }

    fn activate(
        &mut self,
        _: &AudioIOLayout,
        config: &BufferConfig,
        context: &mut impl ActivateContext<Self>,
    ) -> bool {
        if !config.sample_rate.is_finite() || config.sample_rate < 8000.0 {
            return false;
        }

        self.engine = Engine::new(config.sample_rate);
        self.engine.set_lookahead(self.params.lookahead.value());
        self.telemetry.publish(&self.engine);
        context.set_latency_samples(self.engine.latency_samples() as u32);
        true
    }

    fn reset(&mut self) {
        self.engine.reset();
        self.telemetry.publish(&self.engine);
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        auxiliary: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let _denormal_guard = realtime::DenormalGuard::new();
        let old_latency = self.engine.latency_samples();
        self.engine.set_lookahead(self.params.lookahead.value());

        if old_latency != self.engine.latency_samples() {
            context.set_latency_samples(self.engine.latency_samples() as u32);
        }
        self.telemetry
            .audio_channels
            .store(buffer.channels() as u32, Ordering::Relaxed);
        self.telemetry
            .buffer_samples
            .store(buffer.samples() as u32, Ordering::Relaxed);

        self.engine.visualize = self.editor_state.is_open();

        let sidechain = auxiliary.inputs.first();
        if let Some(sidechain) = sidechain {
            if sidechain.samples() != buffer.samples() || !(1..=2).contains(&sidechain.channels()) {
                return ProcessStatus::Error("f0lock: invalid sidechain buffer layout");
            }
        }
        let sidechain = sidechain.map(Buffer::as_slice_immutable);

        for (index, mut frame) in buffer.iter_samples().enumerate() {
            let mode = self.params.mode.value();
            let params = self.params.for_mode(mode);
            let controls = Controls {
                mode,
                bands: std::array::from_fn(|i| params.band(i).control()),
                detection: params.detection.value(),
                target: params.target(),
                tracking_ms: params.tracking.value(),
                attack_ms: params.attack.value(),
                release_ms: params.release.value(),
                clarity: params.clarity.value(),
                gate_db: params.gate.value(),
                mix: params.mix.value() * 0.01,
                output_db: params.output.value(),
                bypass: self.params.bypass.value(),
            };

            let left = *frame.get_mut(0).expect("Missing input channel");
            let right = if frame.len() == 2 {
                *frame.get_mut(1).unwrap()
            } else {
                left
            };

            if !left.is_finite() || !right.is_finite() {
                return ProcessStatus::Error("f0lock: non-finite audio input");
            }

            let detection_input = sidechain.map(|channels| {
                let left = channels[0][index];
                [
                    left,
                    if channels.len() == 2 {
                        channels[1][index]
                    } else {
                        left
                    },
                ]
            });
            if controls.mode == OperatingMode::Sidechain
                && detection_input
                    .is_some_and(|samples| samples.iter().any(|sample| !sample.is_finite()))
            {
                return ProcessStatus::Error("f0lock: non-finite sidechain input");
            }
            let output =
                self.engine
                    .process_with_sidechain([left, right], detection_input, controls);

            if output.iter().any(|sample| !sample.is_finite()) {
                return ProcessStatus::Error("f0lock: non-finite DSP output");
            }

            for (ch, sample) in frame.into_iter().enumerate() {
                *sample = output[ch];
            }
        }

        if self.editor_state.is_open() {
            self.telemetry.publish(&self.engine);
            self.repaint.request_repaint();
        }

        if self.engine.latency_samples() > 0 {
            ProcessStatus::Tail(self.engine.latency_samples() as u32)
        } else {
            ProcessStatus::Normal
        }
    }
}

fn editor_settings() -> EguiNiceSettings {
    #[cfg(target_os = "windows")]
    {
        let mut graphics = nice_plug_egui::GraphicsConfig::default();
        graphics.wgpu_options.surface.present_mode = egui_wgpu::wgpu::PresentMode::AutoNoVsync;
        let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
        setup.instance_descriptor.backends = egui_wgpu::wgpu::Backends::DX12;
        graphics.wgpu_options.wgpu_setup = setup.into();
        EguiNiceSettings::new().with_graphics_config(graphics)
    }

    #[cfg(not(target_os = "windows"))]
    EguiNiceSettings::new()
}

impl ClapPlugin for F0Lock {
    const CLAP_ID: &'static str = "audio.f0lock.eq";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Fundamental-tracking dynamic equalizer");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Equalizer,
        ClapFeature::Mono,
        ClapFeature::Stereo,
    ];
}

impl Vst3Plugin for F0Lock {
    const VST3_CLASS_ID: [u8; 16] = *b"F0LockDynamicEQ1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Eq];
}

nice_export_clap!(F0Lock);
nice_export_vst3!(F0Lock);
