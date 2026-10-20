//! An [`Application`] that receives an [`Instant`] in update logic.

use std::sync::{Arc, Mutex};

use crate::{IcedNiceContext, PersistentState, iced};

use iced::program::{self, Program};
use iced::theme;
use iced::time::Instant;
use iced::window;
use iced::{Element, Settings, Subscription, Task};

use iced_debug as debug;

use super::{Application, BootFn, ViewFn};

/// Creates an [`Application`] with an `update` function that also
/// takes the [`Instant`] of each `Message`.
///
/// This constructor is useful to create animated applications that
/// are _pure_ (e.g. without relying on side-effect calls like [`Instant::now`]).
///
/// Purity is needed when you want your application to end up in the
/// same exact state given the same history of messages. This property
/// enables proper time traveling debugging with [`comet`].
///
/// [`comet`]: https://github.com/iced-rs/comet
pub fn timed<State, PState, Message, Theme, Renderer>(
    editor_state: PersistentState<PState>,
    nice_ctx: IcedNiceContext,
    boot: impl BootFn<State, PState, Message>,
    update: impl UpdateFn<State, Message>,
    subscription: impl Fn(&State) -> Subscription<Message>,
    view: impl for<'a> ViewFn<'a, State, Message, Theme, Renderer>,
) -> Application<impl Program<State = State, Message = (Message, Instant), Theme = Theme>>
where
    State: 'static,
    PState: Send + 'static,
    Message: Send + 'static,
    Theme: theme::Base + 'static,
    Renderer: program::Renderer + 'static,
{
    use std::marker::PhantomData;

    struct Instance<State, PState, Message, Theme, Renderer, Boot, Update, Subscription, View> {
        nice_ctx: IcedNiceContext,
        persistent_state: Arc<Mutex<Option<PState>>>,
        boot: Boot,
        update: Update,
        subscription: Subscription,
        view: View,
        _state: PhantomData<State>,
        _message: PhantomData<Message>,
        _theme: PhantomData<Theme>,
        _renderer: PhantomData<Renderer>,
    }

    impl<State, PState, Message, Theme, Renderer, Boot, Update, Subscription, View> Program
        for Instance<State, PState, Message, Theme, Renderer, Boot, Update, Subscription, View>
    where
        PState: Send + 'static,
        Message: Send + 'static,
        Theme: theme::Base + 'static,
        Renderer: program::Renderer + 'static,
        Boot: self::BootFn<State, PState, Message>,
        Update: self::UpdateFn<State, Message>,
        Subscription: Fn(&State) -> self::Subscription<Message>,
        View: for<'a> self::ViewFn<'a, State, Message, Theme, Renderer>,
    {
        type State = State;
        type Message = (Message, Instant);
        type Theme = Theme;
        type Renderer = Renderer;
        type Executor = iced_futures::backend::default::Executor;

        fn name() -> &'static str {
            let name = std::any::type_name::<State>();

            name.split("::").next().unwrap_or("a_cool_application")
        }

        fn settings(&self) -> Settings {
            Settings::default()
        }

        fn window(&self) -> Option<iced::core::window::Settings> {
            Some(window::Settings::default())
        }

        fn boot(&self) -> (State, Task<Self::Message>) {
            let editor_state = PersistentState::from_shared(&self.persistent_state);

            let (state, task) = self.boot.boot(editor_state, self.nice_ctx.clone());

            (state, task.map(|message| (message, Instant::now())))
        }

        fn update(
            &self,
            state: &mut Self::State,
            (message, now): Self::Message,
        ) -> Task<Self::Message> {
            debug::hot(move || {
                self.update
                    .update(state, message, now)
                    .into()
                    .map(|message| (message, Instant::now()))
            })
        }

        fn view<'a>(
            &self,
            state: &'a Self::State,
            _window: window::Id,
        ) -> Element<'a, Self::Message, Self::Theme, Self::Renderer> {
            debug::hot(|| {
                self.view
                    .view(state)
                    .map(|message| (message, Instant::now()))
            })
        }

        fn subscription(&self, state: &Self::State) -> self::Subscription<Self::Message> {
            debug::hot(|| (self.subscription)(state).map(|message| (message, Instant::now())))
        }
    }

    Application {
        raw: Instance {
            nice_ctx,
            persistent_state: editor_state.into_shared(),
            boot,
            update,
            subscription,
            view,
            _state: PhantomData,
            _message: PhantomData,
            _theme: PhantomData,
            _renderer: PhantomData,
        },
        iced_settings: Settings::default(),
        presets: Vec::new(),
    }
}

/// The update logic of some timed [`Application`].
///
/// This is like [`application::UpdateFn`](super::UpdateFn),
/// but it also takes an [`Instant`].
pub trait UpdateFn<State, Message> {
    /// Processes the message and updates the state of the [`Application`].
    fn update(&self, state: &mut State, message: Message, now: Instant)
    -> impl Into<Task<Message>>;
}

impl<State, Message> UpdateFn<State, Message> for () {
    fn update(
        &self,
        _state: &mut State,
        _message: Message,
        _now: Instant,
    ) -> impl Into<Task<Message>> {
    }
}

impl<T, State, Message, C> UpdateFn<State, Message> for T
where
    T: Fn(&mut State, Message, Instant) -> C,
    C: Into<Task<Message>>,
{
    fn update(
        &self,
        state: &mut State,
        message: Message,
        now: Instant,
    ) -> impl Into<Task<Message>> {
        self(state, message, now)
    }
}
