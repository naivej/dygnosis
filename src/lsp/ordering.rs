//! Receive-order fences for input commits, without serializing independent reads.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::sync::watch;
use tower::Service;
use tower_lsp::jsonrpc::Request;

#[derive(Default)]
struct Progress {
    issued: u64,
    last_input: u64,
    completed: u64,
    finished: BTreeSet<u64>,
}

struct Admission {
    progress: Mutex<Progress>,
    completed: watch::Sender<u64>,
}

impl Admission {
    fn new() -> Self {
        let (completed, _) = watch::channel(0);
        Self {
            progress: Mutex::new(Progress::default()),
            completed,
        }
    }

    fn enter(self: &Arc<Self>, input: bool) -> InputContext {
        let mut progress = self.progress.lock().unwrap();
        let prior = if input {
            progress.issued
        } else {
            progress.last_input
        };
        progress.issued = progress
            .issued
            .checked_add(1)
            .expect("LSP admission overflow");
        let ticket = progress.issued;
        if input {
            progress.last_input = ticket;
        }
        InputContext {
            admission: Arc::clone(self),
            prior,
            ticket,
            input,
        }
    }

    fn finish(&self, ticket: u64) {
        let mut progress = self.progress.lock().unwrap();
        if ticket <= progress.completed {
            return;
        }
        progress.finished.insert(ticket);
        loop {
            let next = progress.completed + 1;
            if !progress.finished.remove(&next) {
                break;
            }
            progress.completed = next;
        }
        self.completed.send_replace(progress.completed);
    }

    async fn wait(&self, ticket: u64) {
        let mut completed = self.completed.subscribe();
        // The sender lives in this admission, so it cannot close during this wait.
        completed.wait_for(|value| *value >= ticket).await.unwrap();
    }
}

struct InputContext {
    admission: Arc<Admission>,
    prior: u64,
    ticket: u64,
    input: bool,
}

impl Drop for InputContext {
    fn drop(&mut self) {
        // Includes invalid parameters, cancellation, and futures dropped by transport.
        self.admission.finish(self.ticket);
    }
}

tokio::task_local! {
    static INPUT: InputContext;
}

/// Called inside tower-lsp's cancellable handler future, before observing input.
pub(super) async fn ready() {
    let context = INPUT.try_with(|input| (Arc::clone(&input.admission), input.prior));
    if let Ok((admission, prior)) = context {
        admission.wait(prior).await;
    }
}

/// A mutation's state is now visible; outgoing publication need not block reads.
pub(super) fn committed() {
    let _ = INPUT.try_with(|input| {
        if input.input {
            input.admission.finish(input.ticket);
        }
    });
}

pub(super) struct OrderedService<S> {
    service: S,
    admission: Arc<Admission>,
}

struct CancellationTask<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for CancellationTask<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl<S> OrderedService<S> {
    pub(super) fn new(service: S) -> Self {
        Self {
            service,
            admission: Arc::new(Admission::new()),
        }
    }
}

fn changes_input(request: &Request) -> bool {
    matches!(
        request.method(),
        "initialize"
            | "initialized"
            | "shutdown"
            | "textDocument/didOpen"
            | "textDocument/didChange"
            | "textDocument/didSave"
            | "textDocument/didClose"
            | "workspace/didChangeWatchedFiles"
            | "workspace/didChangeConfiguration"
            | "workspace/didChangeWorkspaceFolders"
            | "dynare/activeModelChanged"
    ) || (request.method() == "workspace/executeCommand"
        && request
            .params()
            .and_then(|params| params.get("command"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| {
                matches!(command, "dynare/recheckProject" | "dynare/cancelProject")
            }))
}

impl<S> Service<Request> for OrderedService<S>
where
    S: Service<Request>,
    S::Future: Send + 'static,
    S::Response: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, context: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.service.poll_ready(context)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let cancellation = request.method() == "$/cancelRequest"
            && request.id().is_none()
            && request.params().is_some_and(|params| {
                serde_json::from_value::<tower_lsp::lsp_types::CancelParams>(params.clone()).is_ok()
            });
        if cancellation {
            // Registration of prior requests happens synchronously in service.call.
            // Only a valid cancellation notification may bypass saturated handler slots.
            // The returned future owns the task, including if transport drops it unpolled.
            let mut task = CancellationTask(tokio::spawn(self.service.call(request)));
            return Box::pin(async move {
                (&mut task.0)
                    .await
                    .expect("LSP cancellation handler task failed")
            });
        }
        if matches!(request.method(), "$/cancelRequest" | "exit") {
            return Box::pin(self.service.call(request));
        }
        // call runs in frame receive order, before handler futures are polled.
        let input = self.admission.enter(changes_input(&request));
        let future = self.service.call(request);
        Box::pin(INPUT.scope(input, future))
    }
}
