use super::*;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream};
use tokio::time::{Duration, timeout};

struct Wire {
    read: BufReader<tokio::io::ReadHalf<DuplexStream>>,
    write: tokio::io::WriteHalf<DuplexStream>,
    received: Vec<Value>,
}

impl Wire {
    async fn send(&mut self, messages: &[Value]) {
        let mut frames = Vec::new();
        for message in messages {
            let body = serde_json::to_vec(message).unwrap();
            frames.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
            frames.extend_from_slice(&body);
        }
        self.write.write_all(&frames).await.unwrap();
    }

    async fn next(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            assert_ne!(self.read.read_line(&mut line).await.unwrap(), 0);
            if line == "\r\n" {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length: ") {
                length = Some(value.trim().parse::<usize>().unwrap());
            }
        }
        let mut body = vec![0; length.unwrap()];
        self.read.read_exact(&mut body).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    async fn response(&mut self, id: i64) -> Value {
        if let Some(index) = self.received.iter().position(|message| {
            message.get("id") == Some(&json!(id)) && message.get("method").is_none()
        }) {
            return self.received.remove(index);
        }
        loop {
            let message = self.next().await;
            if message.get("id") == Some(&json!(id)) && message.get("method").is_none() {
                return message;
            }
            self.received.push(message);
        }
    }

    async fn server_request(&mut self, method: &str) -> Value {
        if let Some(index) = self
            .received
            .iter()
            .position(|message| message["method"] == method && message.get("id").is_some())
        {
            return self.received.remove(index);
        }
        loop {
            let message = self.next().await;
            if message["method"] == method && message.get("id").is_some() {
                return message;
            }
            self.received.push(message);
        }
    }
}

async fn transport() -> (
    Wire,
    tokio::task::JoinHandle<()>,
    Arc<tokio::sync::Mutex<()>>,
) {
    transport_with_capabilities(json!({})).await
}

async fn transport_with_capabilities(
    capabilities: Value,
) -> (
    Wire,
    tokio::task::JoinHandle<()>,
    Arc<tokio::sync::Mutex<()>>,
) {
    let (service, socket) = new_service();
    let gate = Arc::clone(&service.inner().output_gate);
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (read, write) = tokio::io::split(server);
    let task = tokio::spawn(async move {
        Server::new(read, write, socket)
            .serve(ordering::OrderedService::new(service))
            .await;
    });
    let (read, write) = tokio::io::split(client);
    let mut wire = Wire {
        read: BufReader::new(read),
        write,
        received: Vec::new(),
    };
    wire.send(&[
        json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params": {
            "capabilities":capabilities, "initializationOptions":{"workspaceDiagnostics":false}
        }}),
    ])
    .await;
    assert!(wire.response(1).await.get("result").is_some());
    (wire, task, gate)
}

#[tokio::test]
async fn delayed_refresh_replies_do_not_freeze_incoming_edits_and_requests() {
    // Guards the permanent stall when all four transport slots await refresh
    // replies and a request burst fills the input queue ahead of those replies.
    for (capability, method) in [
        ("semanticTokens", "workspace/semanticTokens/refresh"),
        ("inlayHint", "workspace/inlayHint/refresh"),
    ] {
        let (mut wire, task, _) = transport_with_capabilities(json!({
            "workspace": {capability: {"refreshSupport": true}}
        }))
        .await;
        let uri = "untitled:refresh-stall.mod";
        wire.send(&[open(uri, 1, "before"), symbols(2, uri)]).await;
        assert!(has_symbol(&wire.response(2).await["result"], "before"));

        let mut burst = Vec::new();
        for index in 0..4 {
            burst.push(json!({"jsonrpc":"2.0", "method":"workspace/didChangeConfiguration", "params":{"settings":{"macroWorkLimit":2_000_001 + index}}}));
        }
        // Exceed the transport's bounded input queue while replies are delayed.
        for id in 3..131 {
            burst.push(symbols(id, uri));
        }
        burst.push(json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{
            "textDocument":{"uri":uri,"version":2}, "contentChanges":[{"text":"var after; model; after=0; end;"}]
        }}));
        burst.push(symbols(131, uri));
        wire.send(&burst).await;
        let result = timeout(Duration::from_secs(2), wire.response(131)).await;
        task.abort();
        let result = result.expect("delayed refresh replies froze incoming model work");
        assert!(has_symbol(&result["result"], "after"), "{result}");
        assert!(
            wire.received
                .iter()
                .any(|message| message["method"] == method)
        );
    }
}

#[tokio::test]
async fn refresh_bursts_keep_one_follow_up_and_shutdown_does_not_wait_for_a_reply() {
    // Guards lost refreshes after an in-flight edit, unbounded refresh tasks,
    // and a shutdown that waits for a client which has stopped answering.
    let (mut wire, task, _) = transport_with_capabilities(json!({"workspace": {
        "semanticTokens": {"refreshSupport": true},
        "inlayHint": {"refreshSupport": true}
    }}))
    .await;
    let uri = "untitled:refresh-follow-up.mod";
    wire.send(&[open(uri, 1, "before"), symbols(2, uri)]).await;
    assert!(has_symbol(&wire.response(2).await["result"], "before"));
    let tokens = timeout(
        Duration::from_secs(2),
        wire.server_request("workspace/semanticTokens/refresh"),
    )
    .await
    .unwrap();

    let mut burst: Vec<_> = (0..16)
        .map(|index| {
            json!({
                "jsonrpc":"2.0", "method":"workspace/didChangeConfiguration",
                "params":{"settings":{"parameterValueHints": index % 2 != 0}}
            })
        })
        .collect();
    burst.push(json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{
        "textDocument":{"uri":uri,"version":2}, "contentChanges":[{"text":"var current; model; current=0; end;"}]
    }}));
    burst.push(symbols(100, uri));
    wire.send(&burst).await;
    let result = timeout(Duration::from_secs(2), wire.response(100))
        .await
        .unwrap();
    assert!(has_symbol(&result["result"], "current"), "{result}");
    assert!(
        !wire
            .received
            .iter()
            .any(|message| { message.get("method").is_some() && message.get("id").is_some() })
    );

    // A refused refresh must still release the worker to refresh hints and to
    // consume changes which arrived while the first request was outstanding.
    wire.send(&[json!({"jsonrpc":"2.0", "id":tokens["id"], "error":{
        "code":-32603, "message":"test client refused refresh"
    }})])
    .await;
    for method in [
        "workspace/inlayHint/refresh",
        "workspace/semanticTokens/refresh",
    ] {
        let request = timeout(Duration::from_secs(2), wire.server_request(method))
            .await
            .unwrap();
        wire.send(&[json!({"jsonrpc":"2.0", "id":request["id"], "result":null})])
            .await;
    }
    timeout(
        Duration::from_secs(2),
        wire.server_request("workspace/inlayHint/refresh"),
    )
    .await
    .unwrap();
    wire.send(&[json!({"jsonrpc":"2.0", "id":101, "method":"shutdown"})])
        .await;
    let stopped = timeout(Duration::from_secs(2), wire.response(101)).await;
    task.abort();
    assert_eq!(stopped.unwrap()["result"], Value::Null);
}

#[tokio::test]
async fn prior_open_commits_before_symbols_when_output_is_held() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:ordered.mod";
    wire.send(&[
        json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument": {
            "uri":uri, "languageId":"dynare", "version":1,
            "text":"var unsaved; model; unsaved=0; end;"
        }}}),
        json!({"jsonrpc":"2.0", "id":2, "method":"textDocument/documentSymbol", "params":{"textDocument":{"uri":uri}}}),
    ]).await;
    assert!(
        timeout(Duration::from_millis(100), wire.response(2))
            .await
            .is_err(),
        "a dependent read returned before the prior input could commit"
    );
    drop(held);
    let symbols = timeout(Duration::from_secs(2), wire.response(2))
        .await
        .unwrap();
    assert!(has_symbol(&symbols["result"], "unsaved"), "{symbols}");
    task.abort();
}

fn has_symbol(symbols: &Value, expected: &str) -> bool {
    symbols.as_array().is_some_and(|symbols| {
        symbols
            .iter()
            .any(|symbol| symbol["name"] == expected || has_symbol(&symbol["children"], expected))
    })
}

fn open(uri: &str, version: i32, name: &str) -> Value {
    json!({"jsonrpc":"2.0", "method":"textDocument/didOpen", "params":{"textDocument": {
        "uri":uri, "languageId":"dynare", "version":version,
        "text":format!("var {name}; model; {name}=0; end;")
    }}})
}

fn symbols(id: i64, uri: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":"textDocument/documentSymbol", "params":{"textDocument":{"uri":uri}}})
}

#[tokio::test]
async fn cancellation_bypasses_a_dependent_read_wait() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:cancel-read.mod";
    wire.send(&[
        open(uri, 1, "unsaved"),
        symbols(2, uri),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":2}}),
    ])
    .await;
    let cancelled = timeout(Duration::from_secs(2), wire.response(2))
        .await
        .unwrap();
    assert_eq!(cancelled["error"]["code"], -32800, "{cancelled}");
    wire.send(&[symbols(3, uri)]).await;
    assert!(
        timeout(Duration::from_millis(100), wire.response(3))
            .await
            .is_err()
    );
    drop(held);
    let result = timeout(Duration::from_secs(2), wire.response(3))
        .await
        .unwrap();
    assert!(has_symbol(&result["result"], "unsaved"), "{result}");
    task.abort();
}

#[tokio::test]
async fn cancellation_is_served_when_all_transport_slots_wait_for_input() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:cancel-saturated.mod";
    wire.send(&[
        open(uri, 1, "unsaved"),
        symbols(2, uri),
        symbols(3, uri),
        symbols(4, uri),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":4}}),
    ])
    .await;
    let cancelled = timeout(Duration::from_secs(2), wire.response(4))
        .await
        .unwrap();
    assert_eq!(cancelled["error"]["code"], -32800, "{cancelled}");
    drop(held);
    for id in [2, 3] {
        let result = timeout(Duration::from_secs(2), wire.response(id))
            .await
            .unwrap();
        assert!(has_symbol(&result["result"], "unsaved"), "{result}");
    }
    task.abort();
}

#[tokio::test]
async fn cancellation_registers_for_a_request_queued_beyond_transport_capacity() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:cancel-queued.mod";
    wire.send(&[
        open(uri, 1, "unsaved"),
        symbols(2, uri),
        symbols(3, uri),
        symbols(4, uri),
        symbols(5, uri),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":5}}),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":4}}),
    ])
    .await;
    assert_eq!(
        timeout(Duration::from_secs(2), wire.response(4))
            .await
            .unwrap()["error"]["code"],
        -32800
    );
    assert_eq!(
        timeout(Duration::from_secs(2), wire.response(5))
            .await
            .unwrap()["error"]["code"],
        -32800
    );
    drop(held);
    for id in [2, 3] {
        let result = timeout(Duration::from_secs(2), wire.response(id))
            .await
            .unwrap();
        assert!(has_symbol(&result["result"], "unsaved"), "{result}");
    }
    task.abort();
}

#[tokio::test]
async fn malformed_cancellation_and_cancellation_with_request_id_keep_normal_handling() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:cancel-invalid.mod";
    wire.send(&[
        open(uri, 1, "unsaved"),
        symbols(2, uri),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":{}}}),
        json!({"jsonrpc":"2.0", "id":3, "method":"$/cancelRequest", "params":{"id":2}}),
    ])
    .await;
    let rejected = timeout(Duration::from_secs(2), wire.response(3))
        .await
        .unwrap();
    assert_eq!(rejected["error"]["code"], -32600, "{rejected}");
    assert!(
        timeout(Duration::from_millis(100), wire.response(2))
            .await
            .is_err()
    );
    drop(held);
    let result = timeout(Duration::from_secs(2), wire.response(2))
        .await
        .unwrap();
    assert!(has_symbol(&result["result"], "unsaved"), "{result}");
    task.abort();
}

struct PausedService {
    entered: tokio::sync::mpsc::UnboundedSender<String>,
    release: Arc<tokio::sync::Notify>,
}

impl tower::Service<tower_lsp::jsonrpc::Request> for PausedService {
    type Response = ();
    type Error = std::convert::Infallible;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<(), Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: tower_lsp::jsonrpc::Request) -> Self::Future {
        let method = request.method().to_string();
        let entered = self.entered.clone();
        let release = Arc::clone(&self.release);
        Box::pin(async move {
            ordering::ready().await;
            if method == "textDocument/didOpen" {
                ordering::committed();
            }
            entered.send(method.clone()).unwrap();
            if method == "heldRead" || method == "textDocument/didOpen" {
                release.notified().await;
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn independent_reads_overlap_and_later_input_waits_for_older_reads() {
    use tower::Service;
    use tower_lsp::jsonrpc::Request;
    let (entered, mut events) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let mut service = ordering::OrderedService::new(PausedService {
        entered,
        release: Arc::clone(&release),
    });
    let first = tokio::spawn(service.call(Request::build("heldRead").finish()));
    assert_eq!(events.recv().await.unwrap(), "heldRead");
    let second = tokio::spawn(service.call(Request::build("ordinaryRead").finish()));
    assert_eq!(
        timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap(),
        "ordinaryRead"
    );
    second.await.unwrap().unwrap();
    let input = tokio::spawn(service.call(Request::build("textDocument/didOpen").finish()));
    assert!(
        timeout(Duration::from_millis(100), events.recv())
            .await
            .is_err()
    );
    release.notify_waiters();
    first.await.unwrap().unwrap();
    assert_eq!(
        timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap(),
        "textDocument/didOpen"
    );
    release.notify_waiters();
    input.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn synchronous_provider_does_not_block_polling_an_independent_read() {
    // Guards transport concurrency sharing one polling task: a synchronous
    // model provider must not prevent another admitted read from starting.
    use tower::Service;
    use tower_lsp::jsonrpc::Request;
    struct SynchronousProvider {
        events: tokio::sync::mpsc::UnboundedSender<&'static str>,
        released: Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
    }
    impl Service<Request> for SynchronousProvider {
        type Response = ();
        type Error = std::convert::Infallible;
        type Future = std::pin::Pin<
            Box<dyn std::future::Future<Output = std::result::Result<(), Self::Error>> + Send>,
        >;
        fn poll_ready(
            &mut self,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::result::Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
        fn call(&mut self, request: Request) -> Self::Future {
            let events = self.events.clone();
            let released = Arc::clone(&self.released);
            Box::pin(async move {
                ordering::ready().await;
                if request.method() == "synchronousModelRead" {
                    events.send("model started").unwrap();
                    let _ = released
                        .lock()
                        .unwrap()
                        .recv_timeout(std::time::Duration::from_secs(1));
                    events.send("model finished").unwrap();
                } else {
                    events.send("independent read").unwrap();
                }
                Ok(())
            })
        }
    }
    let (events, mut observed) = tokio::sync::mpsc::unbounded_channel();
    let (release, released) = std::sync::mpsc::channel();
    let mut service = ordering::OrderedService::new(SynchronousProvider {
        events,
        released: Arc::new(Mutex::new(released)),
    });
    let model = service.call(Request::build("synchronousModelRead").finish());
    let read = service.call(Request::build("ordinaryRead").finish());
    let joined = tokio::spawn(async move { tokio::join!(model, read) });
    let first = observed.recv().await.unwrap();
    let second = observed.recv().await.unwrap();
    // Always release the blocking provider before checking the ordering.
    release.send(()).unwrap();
    let (model, read) = joined.await.unwrap();
    model.unwrap();
    read.unwrap();
    assert!(
        first == "independent read" || second == "independent read",
        "synchronous provider monopolized transport polling: {first}, {second}"
    );
}

#[tokio::test]
async fn committed_input_does_not_hold_reads_for_publication_and_dropped_input_releases() {
    use tower::Service;
    use tower_lsp::jsonrpc::Request;
    let (entered, mut events) = tokio::sync::mpsc::unbounded_channel();
    let release = Arc::new(tokio::sync::Notify::new());
    let mut service = ordering::OrderedService::new(PausedService {
        entered,
        release: Arc::clone(&release),
    });
    let input = tokio::spawn(service.call(Request::build("textDocument/didOpen").finish()));
    assert_eq!(events.recv().await.unwrap(), "textDocument/didOpen");
    let read = tokio::spawn(service.call(Request::build("ordinaryRead").finish()));
    assert_eq!(
        timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap(),
        "ordinaryRead"
    );
    read.await.unwrap().unwrap();
    drop(service.call(Request::build("textDocument/didOpen").finish()));
    service
        .call(Request::build("ordinaryRead").finish())
        .await
        .unwrap();
    assert_eq!(events.recv().await.unwrap(), "ordinaryRead");
    release.notify_waiters();
    input.await.unwrap().unwrap();
}

#[tokio::test]
async fn cancelled_and_malformed_mutations_do_not_skip_prior_input_or_wedge_followers() {
    let (mut wire, task, gate) = transport().await;
    let held = gate.lock().await;
    let uri = "untitled:cancel-input.mod";
    wire.send(&[
        open(uri, 1, "unsaved"),
        json!({"jsonrpc":"2.0", "id":2, "method":"workspace/executeCommand", "params":{"command":"dynare/recheckWorkspace"}}),
        json!({"jsonrpc":"2.0", "method":"$/cancelRequest", "params":{"id":2}}),
    ]).await;
    assert_eq!(
        timeout(Duration::from_secs(2), wire.response(2))
            .await
            .unwrap()["error"]["code"],
        -32800
    );
    wire.send(&[
        json!({"jsonrpc":"2.0", "id":3, "method":"workspace/executeCommand", "params":{"command":"dynare/cancelWorkspace", "arguments":false}}),
        symbols(4, uri),
    ]).await;
    assert_eq!(
        timeout(Duration::from_secs(2), wire.response(3))
            .await
            .unwrap()["error"]["code"],
        -32602
    );
    assert!(
        timeout(Duration::from_millis(100), wire.response(4))
            .await
            .is_err()
    );
    drop(held);
    let result = timeout(Duration::from_secs(2), wire.response(4))
        .await
        .unwrap();
    assert!(has_symbol(&result["result"], "unsaved"), "{result}");
    task.abort();
}

#[tokio::test]
async fn ordered_repeated_opens_and_changes_feed_current_hover_pull_and_versions() {
    let (mut wire, task, _) = transport().await;
    let uri = "untitled:versions.mod";
    wire.send(&[
        open(uri, 1, "first"),
        json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{
            "textDocument":{"uri":uri,"version":2}, "contentChanges":[{"text":"var second; model; second=0; end;"}]
        }}),
        open(uri, 3, "third"),
        json!({"jsonrpc":"2.0", "method":"textDocument/didChange", "params":{
            "textDocument":{"uri":uri,"version":4}, "contentChanges":[{"text":"var current; model; current=missing_current; end;"}]
        }}),
        symbols(2, uri),
        json!({"jsonrpc":"2.0", "id":3, "method":"textDocument/hover", "params":{
            "textDocument":{"uri":uri}, "position":{"line":0,"character":5}
        }}),
        json!({"jsonrpc":"2.0", "id":4, "method":"textDocument/diagnostic", "params":{"textDocument":{"uri":uri}}}),
        json!({"jsonrpc":"2.0", "id":5, "method":"workspace/diagnostic", "params":{"previousResultIds":[]}}),
    ]).await;
    let result = timeout(Duration::from_secs(2), wire.response(2))
        .await
        .unwrap();
    assert!(has_symbol(&result["result"], "current"), "{result}");
    for name in ["first", "second", "third"] {
        assert!(!has_symbol(&result["result"], name), "{result}");
    }
    let hover = timeout(Duration::from_secs(2), wire.response(3))
        .await
        .unwrap();
    assert!(
        hover["result"]["contents"]["value"]
            .as_str()
            .unwrap()
            .contains("current"),
        "{hover}"
    );
    let pull = timeout(Duration::from_secs(2), wire.response(4))
        .await
        .unwrap();
    assert!(
        pull["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("missing_current"))
            }),
        "{pull}"
    );
    let workspace = timeout(Duration::from_secs(2), wire.response(5))
        .await
        .unwrap();
    assert!(
        workspace["result"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["uri"] == uri
                    && item["version"] == 4
                    && item["items"] == pull["result"]["items"]
            }),
        "{workspace}"
    );
    // The pull response may arrive first. Wait for the final versioned push too.
    timeout(Duration::from_secs(2), async {
        loop {
            if wire.received.iter().any(|message| {
                message["method"] == "textDocument/publishDiagnostics"
                    && message["params"]["uri"] == uri
                    && message["params"]["version"] == 4
                    && message["params"]["diagnostics"] == pull["result"]["items"]
            }) {
                break;
            }
            let message = wire.next().await;
            wire.received.push(message);
        }
    })
    .await
    .unwrap();
    task.abort();
}
