//! Internal experiment: measure one completed analysis across real tool workers.
//! This module is absent from product builds. Its payload ceiling measures
//! retained text, not the complete heap; it is not a production cache.

use std::cell::RefCell;
use std::sync::{Arc, atomic::Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::source::{RequestControl, SourceInput, SourceSnapshot, sha256};
use crate::engine::analysis_configuration::AnalysisConfiguration;
use crate::engine::language::parser::PARSE_EXPANDED_BUILDS;
use crate::engine::workspace::INPUT_FILE_READS;

const PAYLOAD_LIMIT: usize = 8 * 1024 * 1024;
const IDLE_LIMIT: Duration = Duration::from_secs(30);

struct Entry {
    key: String,
    snapshot: SourceSnapshot,
    captured: Instant,
    payload_bytes: usize,
}

struct State {
    entry: Option<Entry>,
    reused_model: bool,
    reused_report: bool,
    payload_limit: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            entry: None,
            reused_model: false,
            reused_report: false,
            payload_limit: PAYLOAD_LIMIT,
        }
    }
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct Scope;
impl Scope {
    fn new() -> Self {
        STATE.with(|state| assert!(state.replace(Some(State::default())).is_none()));
        Self
    }
}
impl Drop for Scope {
    fn drop(&mut self) {
        STATE.with(|state| state.replace(None));
    }
}

pub(super) struct Checkout {
    key: Option<String>,
    entry: Option<Entry>,
}

impl Checkout {
    pub(super) fn new(
        input: &SourceInput,
        control: &RequestControl,
        analysis: Option<&AnalysisConfiguration>,
        inspection: bool,
        eligible: bool,
    ) -> Self {
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let Some(state) = state.as_mut().filter(|_| eligible && !inspection) else {
                return Self {
                    key: None,
                    entry: None,
                };
            };
            // Full explicit input, policy, choices and work/size ceilings. A
            // matching disk path still goes through the ordinary guard checks.
            let key = sha256(
                serde_json::to_vec(&json!({
                    "source": input, "analysis": analysis,
                    "macro_work_limit": control.macro_work_limit, "limits":control.limits,
                }))
                .unwrap(),
            );
            let entry = state
                .entry
                .take()
                .filter(|entry| entry.key == key && entry.captured.elapsed() < IDLE_LIMIT);
            Self {
                key: Some(key),
                entry,
            }
        })
    }

    pub(super) fn previous(&self) -> Option<(&SourceSnapshot, bool)> {
        self.entry.as_ref().map(|entry| (&entry.snapshot, true))
    }

    pub(super) fn retain(self, snapshot: &SourceSnapshot, control: &RequestControl) {
        let Some(key) = self.key else { return };
        if control.check().is_err() {
            return;
        }
        let payload_bytes = snapshot
            .sources
            .values()
            .map(String::len)
            .sum::<usize>()
            .saturating_add(snapshot.raw.len())
            .saturating_add(snapshot.model.source.len())
            .saturating_add(snapshot.report.effective_text.len());
        STATE.with(|state| {
            let mut state = state.borrow_mut();
            let state = state.as_mut().unwrap();
            state.reused_model = self
                .entry
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(&old.snapshot.model, &snapshot.model));
            state.reused_report = self
                .entry
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(&old.snapshot.report, &snapshot.report));
            if payload_bytes > state.payload_limit {
                return;
            }
            // Copying is inside the timed acquisition. Only immutable analysis
            // Arcs are shared; the next request owns its Workspace and controls.
            let retained = SourceSnapshot {
                source_structure: None,
                workspace: snapshot
                    .workspace
                    .snapshot_with_root_settings(&snapshot.root),
                root: snapshot.root.clone(),
                root_file: snapshot.root_file.clone(),
                owner_roots: snapshot.owner_roots.clone(),
                revision: snapshot.revision.clone(),
                model: Arc::clone(&snapshot.model),
                report: Arc::clone(&snapshot.report),
                raw: snapshot.raw.clone(),
                sources: snapshot.sources.clone(),
                line_indexes: snapshot.line_indexes.clone(),
                analyses: snapshot
                    .analyses
                    .iter()
                    .map(|(root, analysis)| {
                        (
                            root.clone(),
                            super::source::SourceAnalysis {
                                model: Arc::clone(&analysis.model),
                                report: Arc::clone(&analysis.report),
                                coverage: analysis.coverage.clone(),
                            },
                        )
                    })
                    .collect(),
                inputs: snapshot.inputs.clone(),
                guards: snapshot.guards.clone(),
                coverage: snapshot.coverage.clone(),
            };
            state.entry = Some(Entry {
                key,
                snapshot: retained,
                captured: Instant::now(),
                payload_bytes,
            });
        });
    }
}

fn call(tool: &str, arguments: Value) -> Value {
    let control = RequestControl::default();
    let result = crate::engine::control::run(Some(Arc::clone(&control.cancelled)), || {
        super::canonical::dispatch(tool, arguments, control.clone())
    })
    .unwrap()
    .unwrap()
    .structured_content
    .unwrap();
    // Mirror the actual MCP worker's StopWorker disposal after every call.
    control.cancelled.store(true, Ordering::Relaxed);
    result
}

fn facts() -> (usize, usize, bool, bool) {
    STATE.with(|state| {
        let state = state.borrow();
        state.as_ref().map_or((0, 0, false, false), |state| {
            (
                usize::from(state.entry.is_some()),
                state.entry.as_ref().map_or(0, |entry| entry.payload_bytes),
                state.reused_model,
                state.reused_report,
            )
        })
    })
}

#[test]
fn tools_reuse_completed_analysis_with_fresh_controls_and_isolated_inputs() {
    // Guards reuse across different canonical tools after worker disposal,
    // while changed supplied text/map/configuration and Source omission miss.
    let source = json!({"kind":"text","text":"var y;parameters p;p=.8;model;y=p*y(-1);end;"});
    let selected = call(
        "dynare_equations",
        json!({"source":source,"equation":{"kind":"index","scope":"aggregate","index":0}}),
    );
    let jobs = [
        ("dynare_model_info", json!({"source":source})),
        (
            "dynare_equations",
            json!({"source":source,"equation":{"kind":"index","scope":"aggregate","index":0}}),
        ),
        (
            "dynare_expand",
            json!({"source":source,"equation":{"kind":"id","equation_id":selected["data"]["equations"][0]["equation_id"]},"expected_revision":selected["inputs"][0]["revision"]}),
        ),
        (
            "dynare_symbol_info",
            json!({"source":source,"symbol":{"kind":"name","name":"p"}}),
        ),
        ("dynare_diagnose", json!({"source":source})),
    ];
    let baseline: Vec<_> = jobs
        .iter()
        .map(|(tool, args)| {
            PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
            let result = call(tool, args.clone());
            (result, PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get))
        })
        .collect();
    let _scope = Scope::new();
    for (index, ((tool, args), (expected, baseline_builds))) in
        jobs.iter().zip(baseline).enumerate()
    {
        PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
        assert_eq!(call(tool, args.clone()), expected);
        assert_eq!(facts().0, 1, "retained after {tool}");
        if index > 0 {
            assert!(facts().2 && facts().3, "arcs after {tool}");
            // Supplied Workspace construction still parses its written input.
            // Copying an old Arc does not itself prove fewer parses.
            assert!(
                PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get) <= baseline_builds,
                "{tool}"
            );
        }
    }
    for args in [
        json!({"source":{"kind":"text","text":"var x;model;x=2;end;"}}),
        json!({"source":{"kind":"map","root":"root.mod","files":{"root.mod":source["text"]}}}),
        json!({"source":source,"analysis":{"defines":[{"name":"ALT","expression":"1"}]}}),
        json!({"source":source}),
    ] {
        let result = call("dynare_model_info", args);
        assert_eq!(result["outcome"]["status"], "ok");
        assert!(!facts().2);
    }
}

#[test]
fn eviction_and_expiry_release_the_retained_model() {
    // Guards a retained model surviving replacement, expiry or scope disposal.
    let source = json!({"kind":"text","text":"var y;model;y=0;end;"});
    let scope = Scope::new();
    call("dynare_model_info", json!({"source":source}));
    let old = STATE.with(|state| {
        Arc::downgrade(
            &state
                .borrow()
                .as_ref()
                .unwrap()
                .entry
                .as_ref()
                .unwrap()
                .snapshot
                .model,
        )
    });
    STATE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .unwrap()
            .entry
            .as_mut()
            .unwrap()
            .captured -= IDLE_LIMIT
    });
    call("dynare_model_info", json!({"source":source}));
    assert!(old.upgrade().is_none());
    assert!(!facts().2);
    let replaced = STATE.with(|state| {
        Arc::downgrade(
            &state
                .borrow()
                .as_ref()
                .unwrap()
                .entry
                .as_ref()
                .unwrap()
                .snapshot
                .model,
        )
    });
    call(
        "dynare_model_info",
        json!({"source":{"kind":"text","text":"var z;model;z=1;end;"}}),
    );
    assert!(replaced.upgrade().is_none());
    assert_eq!(facts().0, 1);
    assert!(!facts().2);
    let retained = STATE.with(|state| {
        Arc::downgrade(
            &state
                .borrow()
                .as_ref()
                .unwrap()
                .entry
                .as_ref()
                .unwrap()
                .snapshot
                .model,
        )
    });
    drop(scope);
    assert!(retained.upgrade().is_none());
}

#[test]
fn oversized_retention_recomputes_without_changing_the_tool_result() {
    // Admission only limits retention. It must not refuse a usable request.
    let args = json!({"source":{"kind":"text","text":"var y;model;y=0;end;"}});
    let expected = call("dynare_model_info", args.clone());
    let _scope = Scope::new();
    STATE.with(|state| state.borrow_mut().as_mut().unwrap().payload_limit = 1);
    assert_eq!(call("dynare_model_info", args.clone()), expected);
    assert_eq!(facts().0, 0);
    assert_eq!(call("dynare_model_info", args), expected);
    assert_eq!(facts().0, 0);
}

#[cfg(windows)]
fn memory() -> Value {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        faults: u32,
        peak_working_set: usize,
        working_set: usize,
        peak_paged: usize,
        paged: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
        private: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(
            process: *mut std::ffi::c_void,
            created: *mut u64,
            exited: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        cb: std::mem::size_of::<Counters>() as u32,
        ..Default::default()
    };
    // The OS writes only the supplied, correctly sized C-layout counters.
    let success = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<Counters>() as u32,
        )
    };
    assert_ne!(success, 0);
    let mut times = [0u64; 4];
    let [created, exited, kernel, user] = &mut times;
    // FILETIME is an eight-byte pair; each aligned u64 supplies eight writable bytes.
    assert_ne!(
        unsafe { GetProcessTimes(GetCurrentProcess(), created, exited, kernel, user) },
        0
    );
    json!({"private_bytes":counters.private,"working_set_bytes":counters.working_set,"peak_working_set_bytes":counters.peak_working_set,"cpu_ms":(times[2]+times[3]) as f64/10_000.0})
}

#[cfg(not(windows))]
fn memory() -> Value {
    Value::Null
}

#[test]
#[ignore = "manual optimized measurement; needs DYGNOSIS_REUSE_PROBE_INPUT/OUTPUT"]
fn measure_completed_reuse() {
    let input = std::env::var("DYGNOSIS_REUSE_PROBE_INPUT").unwrap();
    let output = std::env::var("DYGNOSIS_REUSE_PROBE_OUTPUT").unwrap();
    let config: Value = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    let scope = config["reuse"].as_bool().unwrap().then(Scope::new);
    let source = &config["source"];
    let common = json!({"source":source,"macro_work_limit":10_000_000});
    let mut equation = common.clone();
    equation["equation"] = json!({"kind":"index","scope":"aggregate","index":0});
    let mut symbol = common.clone();
    symbol["symbol"] = json!({"kind":"name","name":config["symbol"]});
    let jobs = [
        ("overview", "dynare_model_info", common.clone()),
        ("equation", "dynare_equations", equation.clone()),
        ("expand", "dynare_expand", equation),
        ("symbol", "dynare_symbol_info", symbol),
        ("diagnose", "dynare_diagnose", common),
    ];
    let before = memory();
    let mut calls = Vec::new();
    let mut selected = Value::Null;
    for repetition in 0..config["repetitions"].as_u64().unwrap() {
        for (label, tool, args) in &jobs {
            STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.reused_model = false;
                    state.reused_report = false;
                }
            });
            PARSE_EXPANDED_BUILDS.with(|builds| builds.set(0));
            INPUT_FILE_READS.with(|reads| reads.set(0));
            let mut args = args.clone();
            if *label == "expand" {
                args["equation"] = json!({"kind":"id","equation_id":selected["data"]["equations"][0]["equation_id"]});
                args["expected_revision"] = selected["inputs"][0]["revision"].clone();
            }
            let cpu_before = memory()["cpu_ms"].as_f64();
            let start = Instant::now();
            let result = call(tool, args);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let cpu_after = memory()["cpu_ms"].as_f64();
            assert_eq!(result["outcome"]["status"], "ok", "{tool}: {result}");
            if *label == "equation" {
                assert!(!result["data"]["equations"].as_array().unwrap().is_empty());
                selected = result.clone();
            }
            if *label == "expand" {
                assert!(!result["data"]["navigation"].as_array().unwrap().is_empty());
            }
            let (_, _, reused_model, reused_report) = facts();
            calls.push(
                json!({"label":label,"repetition":repetition,"elapsed_ms":elapsed,
                "parse_builds":PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
                "input_reads":INPUT_FILE_READS.with(std::cell::Cell::get),
                "reused_model":reused_model,"reused_report":reused_report,
                "server_cpu_ms":cpu_before.zip(cpu_after).map(|(before,after)|after-before),
                "result_sha256":sha256(serde_json::to_vec(&result).unwrap())}),
            );
        }
    }
    let held = memory();
    let (retained_entries, retained_payload_bytes, _, _) = facts();
    drop(scope);
    let after_drop = memory();
    std::fs::write(output,serde_json::to_vec_pretty(&json!({"calls":calls,
        "memory":{"before":before,"held":held,"after_drop":after_drop},
        "retained_entries":retained_entries,"retained_payload_bytes":retained_payload_bytes,
        "payload_limit":PAYLOAD_LIMIT,"idle_seconds":IDLE_LIMIT.as_secs(),
        "limitations":"One thread-local experimental entry; text payload ceiling excludes model heap; expiry on access; no stdio or concurrent store sharing."
    })).unwrap()).unwrap();
}
