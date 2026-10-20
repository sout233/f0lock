use iced_audio::Gesture;
use nice_plug::{editor::dpi::LogicalSize, prelude::*};
use nice_plug_iced::{
    IcedEditor, IcedEditorState, IcedNiceContext, IcedNiceSettings, PersistentState,
    iced::{
        Center, PollSubNotifier, Subscription, Theme,
        widget::{Column, ProgressBar, column, pick_list, row, text},
    },
};
use nice_plug_iced::{application, create_iced_editor, iced};
use std::sync::{Arc, atomic::Ordering};

const MIN_GAIN_DB: f32 = -30.0;
const MAX_GAIN_DB: f32 = 30.0;

const MIN_WINDOW_SIZE: LogicalSize<f32> = LogicalSize::new(300.0, 300.0);
const RESIZE_HINT: ResizeHint = ResizeHint::resizable().with_min_logical_size(MIN_WINDOW_SIZE);
const INITIAL_SCALE_FACTOR: f32 = 1.0;

/// The time it takes for the peak meter to decay by 12 dB after switching to complete silence.
const PEAK_METER_DECAY_MS: f64 = 150.0;

// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
enum Message {
    /// Sent when the application should poll parameters/meters and redraw.
    Poll,
    WindowResized,
    SetScaleFactor(f32),
    GainGestured(Gesture),
}

/// State relating to the editor itself (not necessarly the GUI). Put any
/// state that should persist between editor opens here.
struct MyEditorState {
    params: Arc<GainParams>,
    peak_meter: Arc<AtomicF32>,
}

struct MyGui {
    /// The editor state is stored inside of a wrapper which allows the
    /// state to persist across editor opens.
    persistent_state: PersistentState<MyEditorState>,

    ctx: IcedNiceContext,

    peak_meter_db: f32,
}

impl MyGui {
    pub fn new(persistent_state: PersistentState<MyEditorState>, ctx: IcedNiceContext) -> Self {
        Self {
            persistent_state,
            ctx,
            peak_meter_db: nice_plug::util::gain_to_db(0.0),
        }
    }

    pub fn theme(&self) -> Option<Theme> {
        Some(Theme::Dark)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch(vec![
            // This subscription is trigged by the `PollSubNotifier`. This can be used,
            // for example, to notify the GUI that parameters have changed or to notify
            // the GUI that it should update its decibel meter.
            iced::poll_events().map(|_| Message::Poll),
            // A subscription that triggers when the window is resized.
            iced::window_resized().map(|_| Message::WindowResized),
        ])
    }

    /// Return the user's scale (zoom) factor. This is applied on top of the system's
    /// scale factor.
    pub fn scale_factor(&self) -> f32 {
        self.ctx.user_scale_factor()
    }

    pub fn update(&mut self, message: Message) {
        let setter = self.ctx.nice_context.param_setter();
        let params = &self.persistent_state.params;

        match message {
            Message::Poll => {
                self.peak_meter_db = nice_plug::util::gain_to_db(
                    self.persistent_state.peak_meter.load(Ordering::Relaxed),
                );
            }
            Message::SetScaleFactor(scale_factor) => {
                self.ctx.set_user_scale_factor(scale_factor);

                // Note, this new scale factor should generally be stored in a config
                // file somewhere so that it persists across sessions.
            }
            Message::WindowResized => {
                // Sync the current window size to the iced editor's state. This is
                // needed to persist window size across editor opens.
                self.ctx.sync_window_size();
            }
            Message::GainGestured(gesture) => {
                iced_audio::param::set_nice_param(&params.gain, gesture, &setter);
            }
        }
    }

    pub fn view(&self) -> Column<'_, Message> {
        use iced_audio::param::nice_to_iced;

        let params = &self.persistent_state.params;

        let knob =
            iced_audio::Knob::new(nice_to_iced(&params.gain)).on_gesture(Message::GainGestured);

        let scale_opts = [
            ScaleOption(0.5),
            ScaleOption(0.75),
            ScaleOption(1.0),
            ScaleOption(1.25),
            ScaleOption(1.5),
            ScaleOption(1.75),
            ScaleOption(2.0),
        ];

        column![
            knob,
            text(
                params
                    .gain
                    .normalized_value_to_string(params.gain.modulated_normalized_value(), true)
            ),
            ProgressBar::new(-80.0..=0.0, self.peak_meter_db),
            row![
                text("scale"),
                pick_list(
                    scale_opts,
                    Some(ScaleOption(self.ctx.user_scale_factor())),
                    |opt| Message::SetScaleFactor(opt.0)
                )
            ]
            .align_y(Center)
            .spacing(7.0)
        ]
        .padding(20)
        .spacing(12.0)
        .align_x(Center)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScaleOption(f32);

impl std::fmt::Display for ScaleOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}%", (self.0 * 100.0).round())
    }
}

// ---------------------------------------------------------------------------------------------------

#[derive(Params)]
pub struct GainParams {
    #[id = "gain"]
    pub gain: FloatParam,
}

impl Default for GainParams {
    fn default() -> Self {
        Self {
            // See the main gain example for more details
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(0.0),
                FloatRange::gain_range(MIN_GAIN_DB, MAX_GAIN_DB),
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(2))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
        }
    }
}

pub struct Gain {
    params: Arc<GainParams>,

    editor_state: Arc<IcedEditorState>,

    /// Needed to normalize the peak meter's response based on the sample rate.
    peak_meter_decay_weight: f32,

    /// The current data for the peak meter. This is stored as an [`Arc`] so we can share it between
    /// the GUI and the audio processing parts. If you have more state to share, then it's a good
    /// idea to put all of that in a struct behind a single `Arc`.
    ///
    /// This is stored as voltage gain.
    peak_meter: Arc<AtomicF32>,

    /// An atomic flag used to notify the program when it should poll for new updates
    /// and redraw (i.e. as a result of the host updating parameters or the audio thread
    /// updating the state of meters). This flag is polled every frame right before
    /// drawing. If the flag is set then the [`poll_events`] subscription will be called, and
    /// the program will update and redraw.
    notifier: PollSubNotifier,
}

impl Default for Gain {
    fn default() -> Self {
        Self {
            params: Arc::new(GainParams::default()),

            // If you wish to make the scale factor user-configurable, then it should be loaded
            // from a config file to make it persist across sessions. The window size however
            // does not need to be stored in a config file because hosts already keep track of
            // that.
            editor_state: IcedEditorState::from_size(MIN_WINDOW_SIZE, INITIAL_SCALE_FACTOR),

            peak_meter_decay_weight: 1.0,
            peak_meter: Arc::new(AtomicF32::new(util::MINUS_INFINITY_DB)),
            notifier: PollSubNotifier::new(),
        }
    }
}

impl Plugin for Gain {
    const NAME: &'static str = "Gain (nice-plug-iced)";
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

    type Editor = IcedEditor;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        create_iced_editor(
            self.editor_state.clone(),
            MyEditorState {
                params: self.params.clone(),
                peak_meter: self.peak_meter.clone(),
            },
            self.notifier.clone(),
            IcedNiceSettings::new().with_resize_hint(RESIZE_HINT),
            |editor_state, nice_ctx| {
                Ok(application(
                    editor_state,
                    nice_ctx,
                    MyGui::new,
                    MyGui::update,
                    MyGui::view,
                )
                .theme(MyGui::theme)
                .scale_factor(MyGui::scale_factor)
                .subscription(MyGui::subscription)
                .run())
            },
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
                let current_peak_meter = self.peak_meter.load(Ordering::Relaxed);
                let mut new_peak_meter = if amplitude > current_peak_meter {
                    amplitude
                } else {
                    current_peak_meter * self.peak_meter_decay_weight
                        + amplitude * (1.0 - self.peak_meter_decay_weight)
                };
                if new_peak_meter < 0.0001 {
                    new_peak_meter = 0.0;
                }

                if current_peak_meter != new_peak_meter {
                    self.peak_meter.store(new_peak_meter, Ordering::Relaxed);

                    // Notify the GUI that it should redraw.
                    self.notifier.notify();
                }
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Gain {
    const CLAP_ID: &'static str = "com.moist-plugins-gmbh-egui.nice-plug-iced-audio-widget";
    const CLAP_DESCRIPTION: Option<&'static str> =
        Some("A smoothed gain parameter example plugin with Iced GUI");
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
    const VST3_CLASS_ID: [u8; 16] = *b"IcedAudioYeahBoy";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Tools];
}

nice_export_clap!(Gain);
nice_export_vst3!(Gain);
