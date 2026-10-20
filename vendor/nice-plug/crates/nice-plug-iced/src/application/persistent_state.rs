use std::{
    fmt::Debug,
    ops::{Deref, DerefMut},
    sync::{Arc, Mutex},
};

/// A special wrapper for state that persists between editor opens. (Not between
/// DAW sessions.)
pub struct PersistentState<State: 'static + Send> {
    shared: Arc<Mutex<Option<State>>>,
    owned: Option<State>,
}

impl<State: 'static + Send> PersistentState<State> {
    pub(crate) fn from_shared(shared: &Arc<Mutex<Option<State>>>) -> Self {
        let owned = shared.lock().unwrap().take().unwrap();
        Self {
            shared: Arc::clone(shared),
            owned: Some(owned),
        }
    }

    pub(crate) fn into_shared(mut self) -> Arc<Mutex<Option<State>>> {
        if let Some(owned) = self.owned.take() {
            *self.shared.lock().unwrap() = Some(owned);
        }

        Arc::clone(&self.shared)
    }
}

impl<State: 'static + Send> AsRef<State> for PersistentState<State> {
    fn as_ref(&self) -> &State {
        // Safety: The `from_shared` constructor ensures that this is always `Some`.
        unsafe { self.owned.as_ref().unwrap_unchecked() }
    }
}

impl<State: 'static + Send> AsMut<State> for PersistentState<State> {
    fn as_mut(&mut self) -> &mut State {
        // Safety: The `from_shared` constructor ensures that this is always `Some`.
        unsafe { self.owned.as_mut().unwrap_unchecked() }
    }
}

impl<State: 'static + Send> Deref for PersistentState<State> {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<State: 'static + Send> DerefMut for PersistentState<State> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

impl<State: 'static + Send> Drop for PersistentState<State> {
    fn drop(&mut self) {
        if let Some(owned) = self.owned.take()
            && let Ok(mut shared) = self.shared.lock()
        {
            *shared = Some(owned);
        }
    }
}

impl<State: 'static + Send + Debug> Debug for PersistentState<State> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.owned.as_ref().unwrap().fmt(f)
    }
}
