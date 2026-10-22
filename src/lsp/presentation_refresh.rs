//! Coalesce editor refresh requests without occupying incoming handler slots.

use std::sync::{Arc, Mutex};

use tokio::task::AbortHandle;
use tower_lsp::Client;

#[derive(Default)]
struct Pending {
    tokens: bool,
    hints: bool,
    stopped: bool,
    worker: Option<AbortHandle>,
}

#[derive(Default)]
pub(super) struct PresentationRefresh {
    pending: Arc<Mutex<Pending>>,
}

impl PresentationRefresh {
    pub fn request(&self, client: &Client, tokens: bool, hints: bool) {
        if !tokens && !hints {
            return;
        }
        let mut pending = self.pending.lock().unwrap();
        if pending.stopped {
            return;
        }
        pending.tokens |= tokens;
        pending.hints |= hints;
        if pending.worker.is_some() {
            return;
        }
        let state = Arc::clone(&self.pending);
        let client = client.clone();
        let task = tokio::spawn(async move {
            loop {
                let (tokens, hints) = {
                    let mut pending = state.lock().unwrap();
                    if !pending.tokens && !pending.hints {
                        pending.worker = None;
                        return;
                    }
                    (
                        std::mem::take(&mut pending.tokens),
                        std::mem::take(&mut pending.hints),
                    )
                };
                // A client reply must never hold one of the transport's incoming
                // slots: a full input queue can otherwise block that very reply.
                if tokens {
                    let _ = client.semantic_tokens_refresh().await;
                }
                if hints {
                    let _ = client.inlay_hint_refresh().await;
                }
            }
        });
        pending.worker = Some(task.abort_handle());
    }

    pub fn stop(&self) {
        let mut pending = self.pending.lock().unwrap();
        pending.stopped = true;
        pending.tokens = false;
        pending.hints = false;
        if let Some(worker) = pending.worker.take() {
            worker.abort();
        }
    }
}

impl Drop for PresentationRefresh {
    fn drop(&mut self) {
        self.stop();
    }
}
