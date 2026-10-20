use egui::{Margin, Vec2};
use nice_plug::{context::gui::GuiContext, editor::dpi::LogicalSize, prelude::*};
use nice_plug_egui::{
    EguiEditor, EguiEditorState, EguiNiceSettings, NiceEguiApp, RepaintNotifier,
    create_egui_editor, resizable_window::ResizableWindow, widgets,
};
use std::sync::Arc;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 300.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);
const INITIAL_ZOOM_FACTOR: f32 = 1.0;

/// The time it takes for the peak meter to decay by 12 dB after switching to complete silence.
const PEAK_METER_DECAY_MS: f64 = 150.0;

/// If you are using message channels, then allocate enough capacity for the expected worse case
/// scenario for your plugin.
const GUI_TO_AUDIO_MSG_CHANNEL_CAPACITY: usize = 512;
const AUDIO_TO_GUI_MSG_CHANNEL_CAPACITY: usize = 128;

// ---------------------------------------------------------------------------------------------------

/// Here you can store any state you need for your GUI.
///
/// This state persists across editor openings.
pub struct GainEditor {
    open_state: Option<OpenEditorState>,
    zoom_factor: f32,
    track_info: Option<TrackInfo>,

    params: Arc<GainParams>,
    peak_meter: Arc<AtomicF32>,

    // For some reason, softbuffer doesn't show anything on the first paint.
    /// A message channel to send events between the GUI and the audio thread.
    ///
    /// This is optional. If you don't need to pass events, you can omit this field.
    msg_channel: GuiMsgChannel,

    /// State that is synced between the GUI and the audio thread using a triple buffer.
    /// This can be used as an alternative the message channel approach. Note, the roles of which
    /// thread has the input and which has the output can be reversed.
    ///
    /// The downside to this approach is that it takes 3x the memory.
    ///
    /// This is optional. If you don't need this, you can omit it.
    triple_buffer_state: triple_buffer::Input<TripleBufferState>,
    next_value: u64,
}

/// Editor-related state that only exists while an editor window is open.
struct OpenEditorState {
    _egui_ctx: egui::Context,
    nice_gui_ctx: GuiContext,
    is_dragging_slider: bool,
}

impl OpenEditorState {
    fn new(egui_ctx: egui::Context, nice_gui_ctx: GuiContext) -> Self {
        Self {
            _egui_ctx: egui_ctx,
            nice_gui_ctx,
            is_dragging_slider: false,
        }
    }
}

impl NiceEguiApp for GainEditor {
    fn build(
        &mut self,
        egui_ctx: egui::Context,
        nice_gui_ctx: GuiContext,
        _frame: &mut nice_plug_egui::Frame,
    ) -> Result<(), nice_plug_egui::baseview::HandlerError> {
        self.open_state = Some(OpenEditorState::new(egui_ctx, nice_gui_ctx));
        Ok(())
    }

    /// Called when the editor is closed. This is needed because the plugin editor window
    /// can be opened and closed multiple times.
    ///
    /// If your app holds onto an `egui::Context` context, then all copies of that context
    /// should be dropped so that egui can properly clean up.
    fn editor_closed(&mut self) {
        self.open_state = None;
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut nice_plug_egui::Frame) {
        let Some(state) = self.open_state.as_mut() else {
            return;
        };

        let setter = state.nice_gui_ctx.param_setter();

        ResizableWindow::new("res-wind")
            .min_size(Vec2::new(MIN_WINDOW_SIZE.width, MIN_WINDOW_SIZE.height))
            .show(ui, |ui| {
                egui::Frame::new()
                    .inner_margin(Margin::same(5))
                    .show(ui, |ui| {
                        // Display the track information
                        let track_info = self.track_info.clone().unwrap_or_default();
                        let name = track_info.name();
                        if name.is_empty() {
                            ui.label("Track name: (unknown)");
                        } else {
                            ui.label(format!("Track name: {name}"));
                        }
                        if let Some(color) = track_info.color() {
                            let (r, g, b, a) = color.rgba();

                            ui.horizontal(|ui| {
                                ui.label("Track color: ");
                                let (rect, _response) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 16.0),
                                    egui::Sense::hover(),
                                );

                                ui.painter().rect_filled(
                                    rect,
                                    2.0,
                                    egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                                );
                            });
                        }

                        // This is a fancy widget that can get all the information it needs to properly
                        // display and modify the parameter from the parametr itself
                        // It's not yet fully implemented, as the text is missing.
                        ui.label("Some random integer");
                        ui.add(widgets::ParamSlider::for_param(
                            &self.params.some_int,
                            &setter,
                        ));

                        ui.label("Gain");
                        ui.add(widgets::ParamSlider::for_param(&self.params.gain, &setter));

                        ui.label(
                            "Also gain, but with a standard widget. Note that it doesn't properly \
                             take the parameter curve into account!",
                        );

                        // This is a simple naive version of a parameter slider that's not aware of how
                        // the parameters work
                        let prev_value = nice_plug::util::gain_to_db(self.params.gain.value());
                        let mut new_value = prev_value;
                        let ptr_down = ui
                            .add(
                                egui::widgets::Slider::new(&mut new_value, -30.0..=30.0)
                                    .suffix(" dB"),
                            )
                            .is_pointer_button_down_on();
                        if !state.is_dragging_slider && (ptr_down || new_value != prev_value) {
                            state.is_dragging_slider = true;
                            setter.begin_set_parameter(&self.params.gain);
                        }
                        if new_value != prev_value {
                            setter.set_parameter(
                                &self.params.gain,
                                nice_plug::util::db_to_gain(new_value),
                            );
                        }
                        if state.is_dragging_slider && !ptr_down {
                            state.is_dragging_slider = false;
                            setter.end_set_parameter(&self.params.gain);
                        }

                        // TODO: Add a proper custom widget instead of reusing a progress bar
                        let peak_meter = util::gain_to_db(
                            self.peak_meter.load(std::sync::atomic::Ordering::Relaxed),
                        );

                        let peak_meter_text = if peak_meter > util::MINUS_INFINITY_DB {
                            format!("{peak_meter:.1} dBFS")
                        } else {
                            String::from("-inf dBFS")
                        };

                        let mut peak_meter_normalized = (peak_meter + 60.0) / 60.0;
                        if peak_meter_normalized <= util::MINUS_INFINITY_GAIN {
                            peak_meter_normalized = 0.0;
                        } else {
                            // Smooth the meter value on the next frame.
                            ui.request_repaint();
                        }

                        ui.allocate_space(egui::Vec2::splat(2.0));
                        ui.add(
                            egui::widgets::ProgressBar::new(peak_meter_normalized)
                                .text(peak_meter_text),
                        );

                        // Demonstrate sending a message to the audio thread.
                        if ui.button("send message").clicked()
                            && let Err(e) =
                                self.msg_channel.to_audio_tx.push(GuiToAudioMsg::MessageA)
                        {
                            nice_error!("Failed to send message to audio thread: {}", e);
                        }
                        // Demonstrate receiving messages from the audio thread.
                        while let Ok(msg) = self.msg_channel.from_audio_rx.pop() {
                            nice_log!("Got message from audio thread: {:?}", &msg);
                        }

                        // Demonstrate mutating synced triple buffer state.
                        if ui.button("mutate synced state").clicked() {
                            self.next_value += 1;
                            // Note, `triple_buffer_state.input_buffer_mut()` will not work for syncing state
                            // this way. You must always completely overwrite the state with new data.
                            self.triple_buffer_state.write(TripleBufferState {
                                value_a: false,
                                value_b: self.next_value,
                                some_data: Vec::new(),
                            });
                        }

                        let before = self.zoom_factor;
                        egui::ComboBox::from_label("zoom factor")
                            .selected_text(format!(
                                "{}%",
                                (self.zoom_factor * 100.0).round() as u32
                            ))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.zoom_factor, 0.5, "50%");
                                ui.selectable_value(&mut self.zoom_factor, 0.75, "75%");
                                ui.selectable_value(&mut self.zoom_factor, 1.0, "100%");
                                ui.selectable_value(&mut self.zoom_factor, 1.25, "125%");
                                ui.selectable_value(&mut self.zoom_factor, 1.5, "150%");
                                ui.selectable_value(&mut self.zoom_factor, 1.75, "175%");
                                ui.selectable_value(&mut self.zoom_factor, 2.0, "200%");
                            });
                        if self.zoom_factor != before {
                            ui.set_zoom_factor(self.zoom_factor);
                        }
                    });
            });
    }

    fn track_info_changed(&mut self, info: TrackInfo) {
        self.track_info = Some(info);
    }
}

// ---------------------------------------------------------------------------------------------------

/// A message channel to send events between the GUI and the audio thread.
///
/// This is optional. If you don't need to pass events, you can omit this.
pub struct GuiMsgChannel {
    /// A message channel to send events from the GUI to the audio thread.
    to_audio_tx: rtrb::Producer<GuiToAudioMsg>,
    /// A message channel to receive events from the audio thread.
    from_audio_rx: rtrb::Consumer<AudioToGuiMsg>,
}
/// A message channel to send events between the GUI and the audio thread.
///
/// This is optional. If you don't need to pass events, you can omit this.
pub struct AudioMsgChannel {
    /// A message channel to send events from the audio thread to the GUI thread.
    to_gui_tx: rtrb::Producer<AudioToGuiMsg>,
    /// A message channel to receive events from the GUI thread.
    from_gui_rx: rtrb::Consumer<GuiToAudioMsg>,
    msg_sent: bool,
}

#[derive(Debug)]
pub enum GuiToAudioMsg {
    MessageA,
    MessageWithHeapData(Vec<f32>),
}
#[derive(Debug)]
pub enum AudioToGuiMsg {
    MessageA,
    DropOldHeapData(Vec<f32>),
}

/// State that is synced between the GUI and the audio thread using a triple buffer.
/// This can be used as an alternative the message channel approach. Note, the roles
/// of which thread has the input and which has the output can be reversed.
///
/// The downside to this approach is that it takes 3x the memory.
///
/// This is optional. If you don't need this, you can omit it.
#[derive(Debug, Default, Clone)]
pub struct TripleBufferState {
    value_a: bool,
    value_b: u64,
    some_data: Vec<u32>,
}

// ---------------------------------------------------------------------------------------------------

#[derive(Params)]
pub struct GainParams {
    #[id = "gain"]
    pub gain: FloatParam,

    // TODO: Remove this parameter when we're done implementing the widgets
    #[id = "foobar"]
    pub some_int: IntParam,
}

impl Default for GainParams {
    fn default() -> Self {
        Self {
            // See the main gain example for more details
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::gain_range(-30.0, 30.0),
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            some_int: IntParam::new("Something", 3, IntRange::Linear { min: 0, max: 3 }),
        }
    }
}

pub struct Gain {
    params: Arc<GainParams>,

    editor_state: Arc<EguiEditorState>,

    /// Needed to normalize the peak meter's response based on the sample rate.
    peak_meter_decay_weight: f32,
    /// The current data for the peak meter. This is stored as an [`Arc`] so we can share it between
    /// the GUI and the audio processing parts. If you have more state to share, then it's a good
    /// idea to put all of that in a struct behind a single `Arc`.
    ///
    /// This is stored as voltage gain.
    peak_meter: Arc<AtomicF32>,

    /// A realtime-safe handle to request a repaint & update for an egui app.
    ///
    /// This can be used, for example, to notify the GUI that the value of a decibel
    /// meter has changed.
    repaint_notifier: RepaintNotifier,

    /// A message channel to send events between the GUI and the audio thread.
    ///
    /// This is optional. If you don't need to pass events, you can omit this field.
    msg_channel: AudioMsgChannel,
    /// Used to demonstrate how to pass heap-allocated data from the GUI to the audio thread.
    heap_data_example: Vec<f32>,

    /// State that is synced between the GUI and the audio thread using a triple buffer.
    /// This can be used as an alternative to the message channel approach. Note, the roles of which
    /// thread has the input and which has the output can be reversed.
    ///
    /// The downside to this approach is that it takes 3x the memory.
    ///
    /// This is optional. If you don't need this, you can omit it.
    triple_buffer_state: triple_buffer::Output<TripleBufferState>,

    /// Temporarily hold on to the initial GUI state until the editor is first opened.
    initial_editor: Option<GainEditor>,
}

impl Default for Gain {
    fn default() -> Self {
        let (to_audio_tx, from_gui_rx) = rtrb::RingBuffer::new(GUI_TO_AUDIO_MSG_CHANNEL_CAPACITY);
        let (to_gui_tx, from_audio_rx) = rtrb::RingBuffer::new(AUDIO_TO_GUI_MSG_CHANNEL_CAPACITY);

        let (triple_buffer_input, triple_buffer_output) =
            triple_buffer::triple_buffer(&TripleBufferState::default());

        let params = Arc::new(GainParams::default());
        let peak_meter = Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB));

        // If you wish to make the zoom factor user-configurable, then it should be loaded
        // from a config file to make it persistent. The window size however does not need
        // to be stored in a config file because hosts already keep track of that.
        let zoom_factor = INITIAL_ZOOM_FACTOR;

        let initial_editor = GainEditor {
            open_state: None,
            track_info: None,
            params: params.clone(),
            peak_meter: peak_meter.clone(),
            msg_channel: GuiMsgChannel {
                to_audio_tx,
                from_audio_rx,
            },
            triple_buffer_state: triple_buffer_input,
            next_value: 0,
            zoom_factor,
        };

        Self {
            params,

            editor_state: EguiEditorState::from_size(MIN_WINDOW_SIZE, zoom_factor),

            peak_meter_decay_weight: 1.0,
            peak_meter,

            repaint_notifier: RepaintNotifier::new(),

            msg_channel: AudioMsgChannel {
                to_gui_tx,
                from_gui_rx,
                msg_sent: false,
            },
            heap_data_example: Vec::new(),

            triple_buffer_state: triple_buffer_output,

            initial_editor: Some(initial_editor),
        }
    }
}

impl Plugin for Gain {
    const NAME: &'static str = "Gain (nice-plug-egui)";
    const VENDOR: &'static str = "Moist Plugins GmbH";
    const URL: &'static str = "https://youtu.be/dQw4w9WgXcQ";
    const EMAIL: &'static str = "info@example.com";

    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
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

    type Editor = EguiEditor<GainEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_egui_editor(
            self.editor_state.clone(),
            self.repaint_notifier.clone(),
            EguiNiceSettings::new().with_resize_hint(RESIZE_HINT),
            self.initial_editor.take().unwrap(),
        )
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        // After `PEAK_METER_DECAY_MS` milliseconds of pure silence, the peak meter's value should
        // have dropped by 12 dB
        self.peak_meter_decay_weight = 0.25f64
            .powf((buffer_config.sample_rate as f64 * PEAK_METER_DECAY_MS / 1000.0).recip())
            as f32;

        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        // Demonstrate receiving messages from the GUI thread.
        while let Ok(msg) = self.msg_channel.from_gui_rx.pop() {
            match msg {
                GuiToAudioMsg::MessageA => {
                    nice_dbg!("Got MessageA from GUI");
                }
                GuiToAudioMsg::MessageWithHeapData(mut heap_data) => {
                    nice_dbg!("Got MessageWithHeapData from GUI");

                    // Replace the old heap data with the new data.
                    std::mem::swap(&mut self.heap_data_example, &mut heap_data);

                    // Note, you must be careful not to drop heap-allocated data on the audio
                    // thread. Send the old data back to the GUI thread to be deallocated there.
                    if let Err(e) = self
                        .msg_channel
                        .to_gui_tx
                        .push(AudioToGuiMsg::DropOldHeapData(heap_data))
                    {
                        nice_error!("Failed to send message to GUI thread: {}", e);
                    }
                }
            }
        }

        // Demonstrate sending messages to the GUI thread.
        if self.editor_state.is_open() && !self.msg_channel.msg_sent {
            if let Err(e) = self.msg_channel.to_gui_tx.push(AudioToGuiMsg::MessageA) {
                nice_error!("Failed to send message to GUI thread: {}", e);
            }

            // Only send the example message once to avoid spamming the GUI.
            self.msg_channel.msg_sent = true;
        }

        // Demonstrate triple buffer usage.
        let state = self.triple_buffer_state.read();
        // Use the state somehow...
        let _ = &state.value_a;
        let _ = &state.value_b;
        let _ = &state.some_data;

        for channel_samples in buffer.iter_samples() {
            let mut amplitude = 0.0;
            let num_samples = channel_samples.len();

            let gain = self.params.gain.smoothed.next();
            for sample in channel_samples {
                *sample *= gain;
                amplitude += *sample;
            }

            // To save resources, a plugin can (and probably should!) only perform expensive
            // calculations that are only displayed on the GUI while the GUI is open
            if self.editor_state.is_open() {
                amplitude = (amplitude / num_samples as f32).abs();
                let current_peak_meter = self.peak_meter.load(std::sync::atomic::Ordering::Relaxed);
                let mut new_peak_meter = if amplitude > current_peak_meter {
                    amplitude
                } else {
                    current_peak_meter * self.peak_meter_decay_weight
                        + amplitude * (1.0 - self.peak_meter_decay_weight)
                };
                if new_peak_meter <= util::MINUS_INFINITY_GAIN {
                    new_peak_meter = 0.0;
                }

                if new_peak_meter != current_peak_meter {
                    self.peak_meter
                        .store(new_peak_meter, std::sync::atomic::Ordering::Relaxed);
                    self.repaint_notifier.request_repaint();
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Gain {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh-egui.nice-plug-gain-egui";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("A smoothed gain parameter example plugin");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Utility,
    ];
}

impl Vst3Plugin for Gain {
    const VST3_CLASS_ID: [u8; 16] = *b"GainGuiYeahBoyyy";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(Gain);
nice_export_vst3!(Gain);
