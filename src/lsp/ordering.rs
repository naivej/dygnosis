//! Receive-order fences for input commits, without serializing independent reads.

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use tokio::sync::watch;
use tower::Service;
use tower_lsp::jsonrpc::{Error, Id, Request};
use tower_lsp::lsp_types::{CancelParams, NumberOrString};

#[derive(Default)]
struct Progress {
    issued: u64,
    last_input: u64,
    completed: u64,
    finished: BTreeSet<u64>,
    previews: HashMap<Id, Arc<AtomicBool>>,
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

    fn enter(self: &Arc<Self>, input: bool, preview: Option<Id>) -> InputContext {
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
        let preview = preview.and_then(|id| {
            let std::collections::hash_map::Entry::Vacant(entry) =
                progress.previews.entry(id.clone())
            else {
                return None;
            };
            let cancellation = Arc::new(AtomicBool::new(false));
            entry.insert(Arc::clone(&cancellation));
            Some((id, cancellation))
        });
        InputContext {
            admission: Arc::clone(self),
            prior,
            ticket,
            input,
            preview,
        }
    }

    fn cancel_preview(&self, id: &Id) {
        if let Some(flag) = self.progress.lock().unwrap().previews.get(id) {
            flag.store(true, Ordering::Relaxed);
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
    preview: Option<(Id, Arc<AtomicBool>)>,
}

impl Drop for InputContext {
    fn drop(&mut self) {
        if let Some((id, _)) = &self.preview {
            self.admission.progress.lock().unwrap().previews.remove(id);
        }
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

/// Stop synchronous preview projection when the transport receives a cancel.
pub(super) fn check_preview_cancellation() -> Result<(), Error> {
    let cancelled = INPUT
        .try_with(|input| {
            input
                .preview
                .as_ref()
                .is_some_and(|(_, flag)| flag.load(Ordering::Relaxed))
        })
        .unwrap_or(false);
    if cancelled {
        return Err(Error::request_cancelled());
    }
    Ok(())
}

pub(super) fn preview_cancellation() -> Option<Arc<AtomicBool>> {
    INPUT
        .try_with(|input| input.preview.as_ref().map(|(_, flag)| Arc::clone(flag)))
        .ok()
        .flatten()
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
            | "dynare/outlineActivityChanged"
    ) || (request.method() == "workspace/executeCommand"
        && request
            .params()
            .and_then(|params| params.get("command"))
            .and_then(serde_json::Value::as_str)
            .is_some_and(|command| {
                matches!(
                    command,
                    "dynare/recheckWorkspace"
                        | "dynare/cancelWorkspace"
                        | "dynare/setDocumentContext"
                        | "dynare/setAnalysisConfiguration"
                )
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
        if request.method() == "$/cancelRequest"
            && request.id().is_none()
            && let Some(params) = request
                .params()
                .and_then(|params| serde_json::from_value::<CancelParams>(params.clone()).ok())
        {
            let id = match params.id {
                NumberOrString::Number(number) => Id::Number(i64::from(number)),
                NumberOrString::String(text) => Id::String(text),
            };
            self.admission.cancel_preview(&id);
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
        let preview = (request.method() == "dynare/effectivePreviewHover"
            || request.method() == "workspace/executeCommand"
                && request
                    .params()
                    .and_then(|params| params["command"].as_str())
                    == Some("dynare/showEffectiveModel"))
        .then(|| request.id().cloned())
        .flatten();
        let input = self.admission.enter(changes_input(&request), preview);
        let future = self.service.call(request);
        Box::pin(async move {
            // buffer_unordered admits concurrent futures but polls them in one
            // task. A synchronous provider would otherwise stop its siblings.
            // Transport capacity still bounds tasks; the input ticket stays in
            // the worker until its state is committed or the future is dropped.
            let mut task = CancellationTask(tokio::spawn(INPUT.scope(input, future)));
            (&mut task.0)
                .await
                .expect("LSP request handler task failed")
        })
    }
}
