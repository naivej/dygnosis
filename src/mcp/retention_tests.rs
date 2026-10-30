use super::*;
use crate::mcp::canonical;
use crate::mcp::source;

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Duration;

use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;

const MODEL: &str = "parameters rho; rho=.8; var y; model; y=rho*y(-1); end;";

#[test]
fn retiring_models_stay_charged_and_do_not_block_other_queries() {
    // Guards admitting new idle models before an evicted graph has actually
    // been destroyed, without holding the store lock across that destruction.
    let cache = store(limits(16 * 1024 * 1024, 1));
    run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(MODEL)),
    );
    let old = cache.locked().entries[0].source.model_weak();
    let worker_store = Arc::clone(&cache);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    std::thread::scope(|threads| {
        let worker = threads.spawn(move || {
            RETIRE_HOOK.with(|slot| {
                *slot.borrow_mut() = Some(Box::new(move || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                }))
            });
            run_call(
                Some(&worker_store),
                "dynare_model_info",
                info_args(text_input("var b;model;b=0;end;")),
            )
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let (sibling_tx, sibling_rx) = std::sync::mpsc::channel();
        let sibling_store = Arc::clone(&cache);
        threads.spawn(move || {
            sibling_tx
                .send(run_call(
                    Some(&sibling_store),
                    "dynare_model_info",
                    info_args(text_input("var c;model;c=1;end;")),
                ))
                .unwrap();
        });
        let sibling = sibling_rx.recv_timeout(Duration::from_secs(5));
        let charged = {
            let state = cache.locked();
            assert_eq!(state.retiring, 1);
            assert!(state.entries.is_empty());
            assert!(state.retiring_bytes > 0);
            cache.charge(&state)
        };
        let alive = old.upgrade().is_some();
        release_tx.send(()).unwrap();
        assert_eq!(
            sibling.expect("retirement blocked another query").0["outcome"]["status"],
            "ok"
        );
        assert!(alive);
        assert!(charged <= cache.limits.bytes);
        assert_eq!(worker.join().unwrap().0["outcome"]["status"], "ok");
    });
    assert!(old.upgrade().is_none());
    let state = cache.locked();
    assert_eq!(state.retiring, 0);
    assert_eq!(state.entries.len(), 1);
    assert!(cache.charge(&state) <= cache.limits.bytes);
}

#[test]
fn identity_work_skips_oversized_and_cancelled_inputs_and_stops_mid_hash() {
    // Guards scanning a known oversized source before the resolver can refuse,
    // and cancellation delayed until an entire allowed payload has been hashed.
    let control = RequestControl::default();
    let oversized = text_input(&"x".repeat(control.limits.max_file_bytes + 1));
    assert!(fingerprint(&oversized, &control, None).is_none());
    let allowed = text_input(&"x".repeat(512 * 1024));
    control.cancelled.store(true, Ordering::Relaxed);
    assert!(fingerprint(&allowed, &control, None).is_none());
    control.cancelled.store(false, Ordering::Relaxed);
    crate::engine::control::cancel_after(&control.cancelled, 64);
    assert!(fingerprint(&allowed, &control, None).is_none());
    assert!(control.cancelled.load(Ordering::Relaxed));

    let mut limited = RequestControl::default();
    limited.limits.max_source_bytes = 512;
    let source = text_input(&format!("//{}\nvar y;model;y=0;end;", "\t".repeat(200)));
    assert!(identity_eligible(&source, &limited, None));
    assert!(
        fingerprint(&source, &limited, None).is_none(),
        "serialized identity exceeded its finite budget"
    );
    let cache = store(limits(16 * 1024 * 1024, 1));
    let expected = run_call(None, "dynare_model_info", info_args(source.clone())).0;
    let session = Session::new(&cache);
    let _scope = session.enter();
    limited.retention = Some(Arc::clone(&session));
    let flag = Arc::clone(&limited.cancelled);
    let actual = canonical::dispatch("dynare_model_info", info_args(source), limited);
    session.finish(&actual, &flag);
    assert_eq!(actual.unwrap().structured_content.unwrap(), expected);
    assert!(
        cache.locked().entries.is_empty(),
        "ineligible identity was retained"
    );
}

fn text_input(text: &str) -> SourceInput {
    SourceInput::Text {
        text: text.to_owned(),
    }
}

fn map_input(text: &str) -> SourceInput {
    SourceInput::Map {
        root: "root.mod".into(),
        files: std::collections::BTreeMap::from([("root.mod".into(), text.into())]),
        root_text: None,
        include_dirs: Vec::new(),
        owner_roots: Vec::new(),
    }
}

struct Call {
    result: Result<rmcp::model::CallToolResult, rmcp::ErrorData>,
    hit: bool,
    parse_builds: usize,
    session: Option<Arc<Session>>,
    cancelled: Arc<AtomicBool>,
    _allocations: Option<crate::retained_memory::Scope>,
}

fn prepare_call(store: Option<&Arc<Store>>, tool: &str, arguments: Value) -> Call {
    prepare_call_with_cancelled(store, tool, arguments, Arc::new(AtomicBool::new(false)))
}

fn prepare_call_with_cancelled(
    store: Option<&Arc<Store>>,
    tool: &str,
    arguments: Value,
    cancelled: Arc<AtomicBool>,
) -> Call {
    let session = store.map(Session::new);
    let control = RequestControl {
        cancelled: Arc::clone(&cancelled),
        retention: session.clone(),
        ..Default::default()
    };
    let allocations = session.as_ref().map(|session| session.enter());
    PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
    let result = crate::engine::control::run(Some(Arc::clone(&cancelled)), || {
        canonical::dispatch(tool, arguments, control)
    })
    .map_err(|_| rmcp::ErrorData::internal_error("Request cancelled", None))
    .and_then(|result| result);
    let hit = session.as_ref().is_some_and(|session| session.locked().hit);
    let parse_builds = PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get);
    Call {
        result,
        hit,
        parse_builds,
        session,
        cancelled,
        _allocations: allocations,
    }
}

fn finish(call: Call) -> (Value, bool, usize) {
    if let Some(session) = &call.session {
        session.finish(&call.result, &call.cancelled);
    }
    call.cancelled.store(true, Ordering::Relaxed);
    let result = call.result.expect("canonical request should succeed");
    let value = result
        .structured_content
        .expect("canonical result is structured");
    (value, call.hit, call.parse_builds)
}

fn run_call(store: Option<&Arc<Store>>, tool: &str, arguments: Value) -> (Value, bool, usize) {
    finish(prepare_call(store, tool, arguments))
}

fn info_args(input: SourceInput) -> Value {
    json!({"source":input})
}

fn run_sequence(store: Option<&Arc<Store>>, input: SourceInput) -> Vec<(Value, bool, usize)> {
    let source = serde_json::to_value(&input).unwrap();
    let common = json!({"source":source});
    let mut out = Vec::new();
    out.push(run_call(store, "dynare_model_info", common.clone()));
    let (equations, hit, builds) = run_call(
        store,
        "dynare_equations",
        json!({"source":source,"equation":{"kind":"index","scope":"aggregate","index":0}}),
    );
    let equation = equations["data"]["equations"][0]["equation_id"].clone();
    let revision = equations["inputs"][0]["revision"].clone();
    assert!(!equation.is_null() && !revision.is_null());
    assert!(
        !equations["data"]["equations"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    out.push((equations, hit, builds));
    out.push(run_call(
        store,
        "dynare_expand",
        json!({"source":source,"equation":{"kind":"id","equation_id":equation},"expected_revision":revision}),
    ));
    assert!(
        !out[2].0["data"]["navigation"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    out.push(run_call(
        store,
        "dynare_symbol_info",
        json!({"source":source,"symbol":{"kind":"name","name":"rho"},"sections":["definition","metadata"]}),
    ));
    out.push(run_call(store, "dynare_diagnose", common));
    out
}

fn store(limits: Limits) -> Arc<Store> {
    Store::new(limits, false)
}

fn limits(bytes: usize, entries: usize) -> Limits {
    Limits {
        bytes,
        entries,
        idle: Duration::from_secs(30),
    }
}

struct DiskFixture(std::path::PathBuf);

impl DiskFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let path = std::env::temp_dir().join(format!(
            "dygnosis-retention-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, name: &str, text: &str) {
        std::fs::write(self.0.join(name), text).unwrap();
    }

    fn input(&self, root: &str) -> SourceInput {
        SourceInput::Disk {
            root: self.0.join(root).to_string_lossy().into_owned(),
            include_dirs: Vec::new(),
            owner_roots: Vec::new(),
        }
    }
}

impl Drop for DiskFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn reason_code(value: &Value, code: &str) -> bool {
    value["outcome"]["reasons"]
        .as_array()
        .is_some_and(|reasons| reasons.iter().any(|reason| reason["code"] == code))
}

#[test]
fn completed_text_and_map_sequences_match_uncached_results_and_skip_parse_on_hits() {
    // Guards cached Workspace reuse across MCP tools without changing any canonical result.
    for input in [text_input(MODEL), map_input(MODEL)] {
        let expected = run_sequence(None, input.clone());
        let cache = store(limits(16 * 1024 * 1024, 4));
        let actual = run_sequence(Some(&cache), input);
        assert_eq!(actual.len(), expected.len());
        for (value, _, _) in &actual {
            assert_eq!(value["outcome"]["status"], "ok");
            assert!(
                !value["coverage"].is_null(),
                "request omitted coverage facts"
            );
        }
        for (index, ((value, hit, builds), (reference, _, _))) in
            actual.iter().zip(&expected).enumerate()
        {
            assert_eq!(
                value, reference,
                "canonical response at sequence step {index}"
            );
            assert_eq!(*hit, index != 0, "hit at sequence step {index}");
            if index > 0 {
                assert_eq!(*builds, 0, "reused analysis reparsed at step {index}");
            }
        }
        assert_eq!(cache.locked().entries.len(), 1);
    }
}

#[test]
fn ledger_counts_model_storage_and_rejects_oversized_retention_without_changing_results() {
    // Guards admission using allocated analysis bytes rather than source text length.
    let input = text_input(MODEL);
    let cache = store(limits(16 * 1024 * 1024, 1));
    let (cached, _, _) = run_call(Some(&cache), "dynare_model_info", info_args(input.clone()));
    let entry_bytes = cache.locked().entries[0].owner.bytes();
    assert!(
        entry_bytes > MODEL.len(),
        "ledger counted only the source payload"
    );

    let refused = store(limits(1, 1));
    let expected = run_call(None, "dynare_model_info", info_args(input.clone())).0;
    let (first, hit, _) = run_call(
        Some(&refused),
        "dynare_model_info",
        info_args(input.clone()),
    );
    assert_eq!(first, expected);
    assert!(!hit);
    assert!(refused.locked().entries.is_empty());
    let (second, hit, builds) = run_call(Some(&refused), "dynare_model_info", info_args(input));
    assert_eq!(second, expected);
    assert!(!hit);
    assert!(builds > 0, "oversized result was retained");
    assert!(refused.locked().entries.is_empty());
    assert_eq!(cached, expected);
}

#[test]
fn byte_budget_evicts_a_real_entry_and_releases_its_model() {
    // Guards eviction when two individually admissible allocated entries exceed the byte budget.
    let staging = store(limits(usize::MAX, 4));
    run_call(
        Some(&staging),
        "dynare_model_info",
        info_args(text_input("var a; model; a=0; end;")),
    );
    run_call(
        Some(&staging),
        "dynare_model_info",
        info_args(text_input("var b; model; b=1; end;")),
    );
    let (entry_a, entry_b) = {
        let mut state = staging.locked();
        (state.entries.remove(0), state.entries.remove(0))
    };
    let weak_a = entry_a.source.model_weak();
    let bytes_a = entry_a.owner.bytes();
    let bytes_b = entry_b.owner.bytes();

    let mut bounded = store(limits(usize::MAX, 4));
    let root_bytes = bounded.owner.bytes();
    let byte_budget = root_bytes.saturating_add(bytes_a.max(bytes_b));
    Arc::get_mut(&mut bounded).unwrap().limits.bytes = byte_budget;
    assert!(bounded.publish(entry_a));
    assert!(bounded.publish(entry_b));
    assert_eq!(bounded.locked().entries.len(), 1);
    assert!(
        weak_a.upgrade().is_none(),
        "byte eviction kept the old model alive"
    );
    let state = bounded.locked();
    assert!(state.entries[0].owner.bytes() <= bytes_a.max(bytes_b));
    assert!(bounded.charge(&state) <= byte_budget);
}

#[test]
fn entry_limit_evicts_least_recently_used_model_and_releases_its_owner() {
    // Guards the entry-count bound, LRU recency refresh, and Arc release on eviction.
    let cache = store(limits(16 * 1024 * 1024, 2));
    let model_a = "var a; model; a=0; end;";
    let model_b = "var b; model; b=1; end;";
    let model_c = "var c; model; c=2; end;";
    run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(model_a)),
    );
    run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(model_b)),
    );
    let (weak_a, weak_b) = {
        let state = cache.locked();
        (
            state.entries[0].source.model_weak(),
            state.entries[1].source.model_weak(),
        )
    };
    assert!(
        run_call(
            Some(&cache),
            "dynare_model_info",
            info_args(text_input(model_a))
        )
        .1
    );
    run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(model_c)),
    );
    assert_eq!(cache.locked().entries.len(), 2);
    assert!(
        weak_a.upgrade().is_some(),
        "recently used model was evicted"
    );
    assert!(
        weak_b.upgrade().is_none(),
        "least recently used model remained live"
    );
}

#[test]
fn independent_connection_stores_do_not_share_entries_or_models() {
    // Guards connection ownership: equal source keys in another connection remain cold.
    let first_store = store(limits(16 * 1024 * 1024, 2));
    let second_store = store(limits(16 * 1024 * 1024, 2));
    let input = text_input(MODEL);
    run_call(
        Some(&first_store),
        "dynare_model_info",
        info_args(input.clone()),
    );
    let first_model = first_store.locked().entries[0].source.model_weak();
    let (_, hit, builds) = run_call(Some(&second_store), "dynare_model_info", info_args(input));
    let second_model = second_store.locked().entries[0].source.model_weak();
    assert!(!hit);
    assert!(builds > 0);
    assert!(!first_model.ptr_eq(&second_model));
}

#[tokio::test]
async fn idle_reaper_expires_without_a_request_and_close_rejects_late_publication() {
    // Guards timer-driven expiry and prevents a worker finishing after disconnect from republishing.
    let connection = Connection::new(
        Limits {
            bytes: 16 * 1024 * 1024,
            entries: 2,
            idle: Duration::from_millis(20),
        },
        false,
    );
    let cache = connection.store();
    run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(MODEL)),
    );
    assert_eq!(cache.locked().entries.len(), 1);

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(
        cache.locked().entries.is_empty(),
        "expiry required another request"
    );
    let (_, hit, _) = run_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(MODEL)),
    );
    assert!(!hit, "expired analysis was still reusable");

    let pending = prepare_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input("var z; model; z=3; end;")),
    );
    let pending_session = pending.session.as_ref().unwrap();
    assert!(pending_session.locked().candidate.is_some());
    drop(connection);
    assert!(cache.locked().closed);
    assert!(cache.locked().entries.is_empty());
    pending_session.finish(&pending.result, &pending.cancelled);
    assert!(
        cache.locked().entries.is_empty(),
        "late completion republished after close"
    );
}

#[test]
fn cancelled_request_does_not_publish_completed_source() {
    // Guards cancellation between analysis completion and cache publication.
    let cache = store(limits(16 * 1024 * 1024, 1));
    let pending = prepare_call(
        Some(&cache),
        "dynare_model_info",
        info_args(text_input(MODEL)),
    );
    pending.cancelled.store(true, Ordering::Relaxed);
    pending
        .session
        .as_ref()
        .unwrap()
        .finish(&pending.result, &pending.cancelled);
    assert!(cache.locked().entries.is_empty());
}

#[test]
fn disk_missing_include_change_refuses_old_revision_then_reads_new_facts() {
    // Guards negative lookup candidates: a newly created include cannot reuse stale cached model facts.
    let fixture = DiskFixture::new();
    fixture.write(
        "root.mod",
        "@#include \"child.inc\"\nvar y; model; y=beta*y(-1); end;",
    );
    let input = fixture.input("root.mod");
    let cache = store(limits(16 * 1024 * 1024, 2));
    let (before, _, _) = run_call(Some(&cache), "dynare_model_info", info_args(input.clone()));
    assert_eq!(before["outcome"]["status"], "partial");
    let old_revision = before["inputs"][0]["revision"].clone();
    assert!(!cache.locked().entries.is_empty());

    fixture.write("child.inc", "parameters beta;\n");
    let stale = json!({"source":input,"expected_revision":old_revision});
    let (refused, hit, _) = run_call(Some(&cache), "dynare_model_info", stale);
    assert!(hit, "test did not check out the old cached result");
    assert_eq!(refused["outcome"]["status"], "refused");
    assert!(reason_code(&refused, "INPUT_CHANGED"));
    assert!(
        cache.locked().entries.is_empty(),
        "stale entry was republished"
    );

    let (fresh, _, _) = run_call(Some(&cache), "dynare_model_info", info_args(input.clone()));
    let uncached = run_call(None, "dynare_model_info", info_args(input)).0;
    assert_eq!(fresh, uncached, "fresh read differs from uncached analysis");
    assert_ne!(fresh["inputs"][0]["revision"], old_revision);
    assert_eq!(fresh["outcome"]["status"], "ok");
}

#[test]
fn source_structure_inspection_matches_uncached_result_and_rejects_a_racing_edit() {
    // Guards source inspection after warm reuse and blocks publication when its guarded root changes mid-capture.
    let fixture = DiskFixture::new();
    fixture.write("root.mod", MODEL);
    let input = fixture.input("root.mod");
    let cache = store(limits(16 * 1024 * 1024, 2));
    let (warm, _, _) = run_call(Some(&cache), "dynare_model_info", info_args(input.clone()));
    let old_revision = warm["inputs"][0]["revision"].clone();
    let inspect = json!({"source":input,"sections":["source_structure"]});
    let (cached_inspection, hit, _) = run_call(Some(&cache), "dynare_model_info", inspect.clone());
    let uncached_inspection = run_call(None, "dynare_model_info", inspect.clone()).0;
    assert!(
        hit,
        "source inspection did not use the warm completed analysis"
    );
    assert_eq!(cached_inspection, uncached_inspection);
    assert!(cached_inspection["data"]["source_structure"].is_object());

    let path = fixture.0.join("root.mod");
    let race_path = path.clone();
    let race_args =
        json!({"source":input,"sections":["source_structure"],"expected_revision":old_revision});
    let (raced, hit, _) = source::with_capture_hook(
        source::CaptureBoundary::SourceStructure,
        move || std::fs::write(race_path, "var y; model; y=1; end;\n").unwrap(),
        || run_call(Some(&cache), "dynare_model_info", race_args),
    );
    assert!(hit, "race did not check out the retained source");
    assert_eq!(raced["outcome"]["status"], "refused");
    assert!(reason_code(&raced, "INPUT_CHANGED"));
    assert!(
        cache.locked().entries.is_empty(),
        "racing input facts were published"
    );
}

#[test]
fn inactive_include_declaration_hover_reuses_its_source_only_cache() {
    // Guards a warm include declaration hover avoiding another parser build while preserving effective output.
    let input = SourceInput::Map {
        root: "root.mod".into(),
        files: std::collections::BTreeMap::from([
            (
                "root.mod".into(),
                "@#if 1\nvar mu_cngdp (long_name='Active consumption');\nmodel;\nmu_cngdp=0;\nend;\n@#else\n@#include \"nested/parameters_initial.inc\"\nmodel;\ny=mu_cngdp(1);\ny=pac_expectation(pm)+temp;\nend;\n@#endif\n".into(),
            ),
            (
                "nested/parameters_initial.inc".into(),
                "var mu_cngdp $C$ (long_name='Dormant consumption'); parameters beta; pac_model(model_name=pm,discount=beta); model; #temp=mu_cngdp; end;\n".into(),
            ),
        ]),
        root_text: None,
        include_dirs: Vec::new(),
        owner_roots: Vec::new(),
    };
    let source = serde_json::to_value(&input).unwrap();
    let query = json!({"source":source,"view":"source","symbol":{"kind":"target","file":"root.mod","line":9,"column":5}});
    let effective_query = json!({"source":source});
    let cache = store(limits(64 * 1024 * 1024, 1));
    let expected_effective = run_call(None, "dynare_model_info", effective_query.clone()).0;

    let (cold, cold_hit, cold_builds) = run_call(Some(&cache), "dynare_symbol_info", query.clone());
    assert!(!cold_hit);
    assert_eq!(
        cold["data"]["candidates"].as_array().unwrap().len(),
        1,
        "{cold}"
    );
    assert_eq!(
        cold["data"]["candidates"][0]["long_name"], "Dormant consumption",
        "{cold}"
    );
    assert_eq!(cold["data"]["candidates"][0]["tex_name"], "C", "{cold}");

    let (warm, warm_hit, warm_builds) = run_call(Some(&cache), "dynare_symbol_info", query);
    assert!(warm_hit, "warm include declaration query missed retention");
    assert_eq!(warm, cold);
    assert_eq!(warm_builds, 0, "warm declaration hover rebuilt a parser");
    assert!(cold_builds > 0, "cold request did not exercise parser work");

    let use_line = "y=pac_expectation(pm)+temp;";
    for (name, kind, expression) in [
        ("pm", "pac_model", None),
        ("temp", "model_local_variable", Some("mu_cngdp")),
    ] {
        let args = json!({"source":source,"view":"source","symbol":{"kind":"target","line":10,"column":use_line.find(name).unwrap()+1}});
        let (result, hit, builds) = run_call(Some(&cache), "dynare_symbol_info", args.clone());
        assert!(hit);
        assert_eq!(builds, 0, "typed/local lookup rebuilt written facts");
        assert_eq!(
            result["data"]["candidates"][0]["written_kind"], kind,
            "{result}"
        );
        if let Some(expression) = expression {
            assert_eq!(
                result["data"]["candidates"][0]["expression"], expression,
                "{result}"
            );
        }
        let (again, hit, builds) = run_call(Some(&cache), "dynare_symbol_info", args);
        assert!(hit);
        assert_eq!(builds, 0);
        assert_eq!(again, result);
    }

    let (actual_effective, effective_hit, _) =
        run_call(Some(&cache), "dynare_model_info", effective_query);
    assert!(effective_hit);
    assert_eq!(actual_effective, expected_effective);
}

#[test]
fn cancelled_checkout_does_not_block_a_sibling_or_replace_its_entry() {
    // Guards that a checked-out entry holds no store lock during analysis and cannot erase a sibling publish on cancellation.
    let cache = store(limits(16 * 1024 * 1024, 2));
    let input = text_input(MODEL);
    run_call(Some(&cache), "dynare_model_info", info_args(input.clone()));
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancellation);
    let worker_store = Arc::clone(&cache);
    let worker_input = input.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let (worker_tx, worker_rx) = std::sync::mpsc::channel();
    let (sibling_tx, sibling_rx) = std::sync::mpsc::channel();

    std::thread::scope(|threads| {
        threads.spawn(move || {
            let call = source::with_capture_hook(
                source::CaptureBoundary::AnalysisMetadata,
                move || {
                    ready_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                },
                || {
                    prepare_call_with_cancelled(
                        Some(&worker_store),
                        "dynare_model_info",
                        info_args(worker_input),
                        worker_cancel,
                    )
                },
            );
            let hit = call.hit;
            if let Some(session) = &call.session {
                session.finish(&call.result, &call.cancelled);
            }
            worker_tx.send((hit, call.result.is_err())).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let sibling_store = Arc::clone(&cache);
        let sibling_input = input.clone();
        threads.spawn(move || {
            sibling_tx
                .send(run_call(
                    Some(&sibling_store),
                    "dynare_model_info",
                    info_args(sibling_input),
                ))
                .unwrap();
        });
        let sibling = sibling_rx.recv_timeout(Duration::from_secs(5));
        cancellation.store(true, Ordering::Relaxed);
        release_tx.send(()).unwrap();
        let worker = worker_rx.recv_timeout(Duration::from_secs(5));

        let sibling = sibling.expect("sibling request blocked behind a cache checkout");
        let worker = worker.expect("cancelled worker did not finish after release");
        assert!(!sibling.1, "sibling unexpectedly hit the checked-out entry");
        assert_eq!(sibling.0["outcome"]["status"], "ok");
        assert!(worker.0, "worker did not start from the cached entry");
        assert!(worker.1, "cancelled worker returned an analysis result");
        let state = cache.locked();
        assert_eq!(state.entries.len(), 1);
        assert_eq!(
            state.entries[0].key,
            fingerprint(&input, &RequestControl::default(), None).unwrap()
        );
    });
}
