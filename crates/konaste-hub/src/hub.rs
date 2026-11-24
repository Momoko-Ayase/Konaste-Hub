//! The shared handle between background work and the UI thread.
use crate::{catalog::GameId, state::HubState};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

#[derive(Clone, Debug)]
pub enum Notice {
    Error(&'static str),
    UpdateFound(GameId),
    Installed(GameId),
    GameExited { id: GameId, code: u32 },
    SignedOut,
}

struct Inner {
    state: HubState,
    runtime: tokio::runtime::Handle,
    changed: watch::Sender<u64>,
    notices: mpsc::UnboundedSender<Notice>,
}

#[derive(Clone)]
pub struct Hub(Arc<Inner>);

pub struct Receivers {
    pub changed: watch::Receiver<u64>,
    pub notices: mpsc::UnboundedReceiver<Notice>,
}

impl Hub {
    pub fn new(state: HubState, runtime: tokio::runtime::Handle) -> (Self, Receivers) {
        let (changed, changed_rx) = watch::channel(0);
        let (notices, notices_rx) = mpsc::unbounded_channel();
        (Self(Arc::new(Inner { state, runtime, changed, notices })), Receivers { changed: changed_rx, notices: notices_rx })
    }

    pub fn state(&self) -> &HubState {
        &self.0.state
    }

    /// Ask the UI to redraw from a fresh snapshot.
    pub fn publish(&self) {
        self.0.changed.send_modify(|value| *value = value.wrapping_add(1));
    }

    pub fn notify(&self, notice: Notice) {
        let _ = self.0.notices.send(notice);
        self.publish();
    }

    pub fn spawn<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        self.0.runtime.spawn(future);
    }

    /// Run a fallible action and turn its error into a visible notice.
    pub fn run<F>(&self, future: F)
    where
        F: std::future::Future<Output = Result<(), crate::state::HubError>> + Send + 'static,
    {
        let hub = self.clone();
        self.spawn(async move {
            if let Err(error) = future.await {
                hub.notify(Notice::Error(error.code));
            }
        });
    }
}
