use crate::{IcedNiceContext, iced};

use iced::message;
use iced::program::{self, Program};
use iced::theme;
use iced::window;
use iced::{Element, Executor, Font, Never, Preset, Subscription, Task, Theme};

use iced_debug as debug;

use std::borrow::Cow;
use std::cell::RefCell;

pub mod timed;
pub use timed::timed;

mod persistent_state;
pub use persistent_state::PersistentState;

pub fn application<State, PState, Message, Theme, Renderer>(
    persistent_state: PersistentState<PState>,
    nice_ctx: IcedNiceContext,
    boot: impl BootFn<State, PState, Message>,
    update: impl UpdateFn<State, Message>,
    view: impl for<'a> ViewFn<'a, State, Message, Theme, Renderer>,
) -> Application<impl Program<State = State, Message = Message, Theme = Theme>>
where
    State: 'static,
    PState: Send + 'static,
    Message: Send + 'static + message::MaybeDebug + message::MaybeClone,
    Theme: theme::Base,
    Renderer: program::Renderer,
{
    use std::marker::PhantomData;

    struct Instance<State, PState: Send + 'static, Boot, Message, Theme, Renderer, Update, View> {
        nice_ctx: IcedNiceContext,
        persitent_state: RefCell<Option<PersistentState<PState>>>,
        boot: Boot,
        update: Update,
        view: View,
        _state: PhantomData<State>,
        _message: PhantomData<Message>,
        _theme: PhantomData<Theme>,
        _renderer: PhantomData<Renderer>,
    }

    impl<State, PState, Boot, Message, Theme, Renderer, Update, View> Program
        for Instance<State, PState, Boot, Message, Theme, Renderer, Update, View>
    where
        PState: Send + 'static,
        Message: Send + 'static,
        Theme: theme::Base,
        Renderer: program::Renderer,
        Boot: self::BootFn<State, PState, Message>,
        Update: self::UpdateFn<State, Message>,
        View: for<'a> self::ViewFn<'a, State, Message, Theme, Renderer>,
    {
        type State = State;
        type Message = Message;
        type Theme = Theme;
        type Renderer = Renderer;
        type Executor = iced_futures::backend::default::Executor;

        fn name() -> &'static str {
            let name = std::any::type_name::<State>();

            name.split("::").next().unwrap_or("a_cool_application")
        }

        fn boot(&self) -> (State, Task<Message>) {
            let persistent_state = { self.persitent_state.borrow_mut().take().unwrap() };
            self.boot.boot(persistent_state, self.nice_ctx.clone())
        }

        fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
            self.update.update(state, message)
        }

        fn view<'a>(
            &self,
            state: &'a Self::State,
            _window: window::Id,
        ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
            self.view.view(state)
        }

        fn settings(&self) -> iced::Settings {
            iced::Settings::default()
        }

        fn window(&self) -> Option<iced::core::window::Settings> {
            Some(window::Settings::default())
        }
    }

    Application {
        raw: Instance {
            nice_ctx,
            persitent_state: RefCell::new(Some(persistent_state)),
            boot,
            update,
            view,
            _state: PhantomData,
            _message: PhantomData,
            _theme: PhantomData,
            _renderer: PhantomData,
        },
        iced_settings: iced::Settings::default(),
        presets: Vec::new(),
    }
}

/// The underlying definition and configuration of an iced application.
///
/// You can use this API to create and run iced applications
/// step by step—without coupling your logic to a trait
/// or a specific type.
///
/// You can create an [`Application`] with the [`application`] helper.
pub struct Application<P: Program> {
    raw: P,
    iced_settings: iced::Settings,
    presets: Vec<Preset<P::State, P::Message>>,
}

impl<P: Program + Send> Application<P>
where
    P::Message: message::MaybeDebug + message::MaybeClone,
{
    /// Runs the [`Application`]
    pub fn run(self) -> impl Program
    where
        Self: 'static,
    {
        #[cfg(feature = "debug")]
        iced_debug::init(iced_debug::Metadata {
            name: P::name(),
            theme: None,
            can_time_travel: cfg!(feature = "time-travel"),
        });

        #[cfg(all(feature = "debug", not(target_arch = "wasm32")))]
        let program = iced_devtools::attach(ApplicationInner {
            raw: self.raw,
            iced_settings: self.iced_settings,
            presets: self.presets,
        });

        #[cfg(not(any(all(feature = "debug", not(target_arch = "wasm32")))))]
        let program = ApplicationInner {
            raw: self.raw,
            iced_settings: self.iced_settings,
            presets: self.presets,
        };

        program
    }

    /// Sets the [`Settings`](iced::Settings) that will be used to run the [`Application`].
    pub fn settings(self, settings: iced::Settings) -> Self {
        Self {
            iced_settings: settings,
            ..self
        }
    }

    /// Sets the [`Settings::antialiasing`](iced::Settings) field of the
    /// [`Application`].
    pub fn antialiasing(self, antialiasing: bool) -> Self {
        Self {
            iced_settings: iced::Settings {
                antialiasing,
                ..self.iced_settings
            },
            ..self
        }
    }

    /// Sets the default [`Font`] of the [`Application`].
    pub fn default_font(self, default_font: Font) -> Self {
        Self {
            iced_settings: iced::Settings {
                default_font,
                ..self.iced_settings
            },
            ..self
        }
    }

    /// Adds a font to the list of fonts that will be loaded at the start of the [`Application`].
    pub fn font(mut self, font: impl Into<Cow<'static, [u8]>>) -> Self {
        self.iced_settings.fonts.push(font.into());
        self
    }

    /// Sets the title of the [`Application`].
    pub fn title(
        self,
        title: impl TitleFn<P::State>,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>> {
        Application {
            raw: program::with_title(self.raw, move |state, _window| title.title(state)),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the subscription logic of the [`Application`].
    pub fn subscription(
        self,
        f: impl Fn(&P::State) -> Subscription<P::Message>,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>> {
        Application {
            raw: program::with_subscription(self.raw, f),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the theme logic of the [`Application`].
    pub fn theme(
        self,
        f: impl ThemeFn<P::State, P::Theme>,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>> {
        Application {
            raw: program::with_theme(self.raw, move |state, _window| f.theme(state)),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the style logic of the [`Application`].
    pub fn style(
        self,
        f: impl Fn(&P::State, &P::Theme) -> theme::Style,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>> {
        Application {
            raw: program::with_style(self.raw, f),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the scale factor of the [`Application`]. This is applied on top of the system's
    /// scale factor.
    pub fn scale_factor(
        self,
        f: impl Fn(&P::State) -> f32,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>> {
        Application {
            raw: program::with_scale_factor(self.raw, move |state, _window| f(state)),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the executor of the [`Application`].
    pub fn executor<E>(
        self,
    ) -> Application<impl Program<State = P::State, Message = P::Message, Theme = P::Theme>>
    where
        E: Executor,
    {
        Application {
            raw: program::with_executor::<P, E>(self.raw),
            iced_settings: self.iced_settings,
            presets: self.presets,
        }
    }

    /// Sets the boot presets of the [`Application`].
    ///
    /// Presets can be used to override the default booting strategy
    /// of your application during testing to create reproducible
    /// environments.
    pub fn presets(self, presets: impl IntoIterator<Item = Preset<P::State, P::Message>>) -> Self {
        Self {
            presets: presets.into_iter().collect(),
            ..self
        }
    }
}

/// The underlying definition and configuration of an iced application.
///
/// You can use this API to create and run iced applications
/// step by step—without coupling your logic to a trait
/// or a specific type.
///
/// You can create an [`Application`] with the [`application`] helper.
pub(crate) struct ApplicationInner<P: Program> {
    raw: P,
    iced_settings: iced::Settings,
    presets: Vec<Preset<P::State, P::Message>>,
}

impl<P: Program> Program for ApplicationInner<P> {
    type State = P::State;
    type Message = P::Message;
    type Theme = P::Theme;
    type Renderer = P::Renderer;
    type Executor = P::Executor;

    fn name() -> &'static str {
        P::name()
    }

    fn settings(&self) -> iced::Settings {
        self.iced_settings.clone()
    }

    fn window(&self) -> Option<window::Settings> {
        // Unused by the baseview backend
        Some(window::Settings::default())
    }

    fn boot(&self) -> (Self::State, Task<Self::Message>) {
        self.raw.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Task<Self::Message> {
        debug::hot(|| self.raw.update(state, message))
    }

    fn view<'a>(
        &self,
        state: &'a Self::State,
        window: window::Id,
    ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
        debug::hot(|| self.raw.view(state, window))
    }

    fn title(&self, state: &Self::State, window: window::Id) -> String {
        debug::hot(|| self.raw.title(state, window))
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Self::Message> {
        debug::hot(|| self.raw.subscription(state))
    }

    fn theme(&self, state: &Self::State, window: iced::window::Id) -> Option<Self::Theme> {
        debug::hot(|| self.raw.theme(state, window))
    }

    fn style(&self, state: &Self::State, theme: &Self::Theme) -> theme::Style {
        debug::hot(|| self.raw.style(state, theme))
    }

    fn scale_factor(&self, state: &Self::State, window: window::Id) -> f32 {
        debug::hot(|| self.raw.scale_factor(state, window))
    }

    fn presets(&self) -> &[Preset<Self::State, Self::Message>] {
        &self.presets
    }
}

/// The logic to initialize the `State` of some [`Application`].
///
/// This trait is implemented for both `Fn() -> State` and
/// `Fn() -> (State, Task<Message>)`.
///
/// In practice, this means that [`application`] can both take
/// simple functions like `State::default` and more advanced ones
/// that return a [`Task`].
pub trait BootFn<State, PState: Send + 'static, Message: Send + 'static> {
    /// Initializes the [`Application`] state.
    fn boot(
        &self,
        editor_state: PersistentState<PState>,
        nice_ctx: IcedNiceContext,
    ) -> (State, Task<Message>);
}

impl<T, C, State, PState: Send + 'static, Message: Send + 'static> BootFn<State, PState, Message>
    for T
where
    T: Fn(PersistentState<PState>, IcedNiceContext) -> C,
    C: IntoBoot<State, Message>,
{
    fn boot(
        &self,
        editor_state: PersistentState<PState>,
        nice_ctx: IcedNiceContext,
    ) -> (State, Task<Message>) {
        self(editor_state, nice_ctx).into_boot()
    }
}

/// The initial state of some [`Application`].
pub trait IntoBoot<State, Message> {
    /// Turns some type into the initial state of some [`Application`].
    fn into_boot(self) -> (State, Task<Message>);
}

impl<State, Message> IntoBoot<State, Message> for State {
    fn into_boot(self) -> (State, Task<Message>) {
        (self, Task::none())
    }
}

impl<State, Message> IntoBoot<State, Message> for (State, Task<Message>) {
    fn into_boot(self) -> (State, Task<Message>) {
        self
    }
}

/// The title logic of some [`Application`].
///
/// This trait is implemented both for `&static str` and
/// any closure `Fn(&State) -> String`.
///
/// This trait allows the [`application`] builder to take any of them.
pub trait TitleFn<State> {
    /// Produces the title of the [`Application`].
    fn title(&self, state: &State) -> String;
}

impl<State> TitleFn<State> for &'static str {
    fn title(&self, _state: &State) -> String {
        self.to_string()
    }
}

impl<T, State> TitleFn<State> for T
where
    T: Fn(&State) -> String,
{
    fn title(&self, state: &State) -> String {
        self(state)
    }
}

/// The update logic of some [`Application`].
///
/// This trait allows the [`application`] builder to take any closure that
/// returns any `Into<Task<Message>>`.
pub trait UpdateFn<State, Message> {
    /// Processes the message and updates the state of the [`Application`].
    fn update(&self, state: &mut State, message: Message) -> Task<Message>;
}

impl<State> UpdateFn<State, Never> for () {
    fn update(&self, _state: &mut State, _message: Never) -> Task<Never> {
        Task::none()
    }
}

impl<T, State, Message, C> UpdateFn<State, Message> for T
where
    T: Fn(&mut State, Message) -> C,
    C: Into<Task<Message>>,
{
    fn update(&self, state: &mut State, message: Message) -> Task<Message> {
        self(state, message).into()
    }
}

/// The view logic of some [`Application`].
///
/// This trait allows the [`application`] builder to take any closure that
/// returns any `Into<Element<'_, Message>>`.
pub trait ViewFn<'a, State, Message, Theme, Renderer> {
    /// Produces the widget of the [`Application`].
    fn view(&self, state: &'a State) -> Element<'a, Message, Theme, Renderer>;
}

impl<'a, T, State, Message, Theme, Renderer, Widget> ViewFn<'a, State, Message, Theme, Renderer>
    for T
where
    T: Fn(&'a State) -> Widget,
    State: 'static,
    Widget: Into<Element<'a, Message, Theme, Renderer>>,
{
    fn view(&self, state: &'a State) -> Element<'a, Message, Theme, Renderer> {
        self(state).into()
    }
}

/// The theme logic of some [`Application`].
///
/// Any implementors of this trait can be provided as an argument to
/// [`Application::theme`].
///
/// `iced` provides two implementors:
/// - the built-in [`Theme`] itself
/// - and any `Fn(&State) -> impl Into<Option<Theme>>`.
pub trait ThemeFn<State, Theme> {
    /// Returns the theme of the [`Application`] for the current state.
    ///
    /// If `None` is returned, `iced` will try to use a theme that
    /// matches the system color scheme.
    fn theme(&self, state: &State) -> Option<Theme>;
}

impl<State> ThemeFn<State, Theme> for Theme {
    fn theme(&self, _state: &State) -> Option<Theme> {
        Some(self.clone())
    }
}

impl<F, T, State, Theme> ThemeFn<State, Theme> for F
where
    F: Fn(&State) -> T,
    T: Into<Option<Theme>>,
{
    fn theme(&self, state: &State) -> Option<Theme> {
        (self)(state).into()
    }
}
