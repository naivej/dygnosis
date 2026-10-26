//! Connection-owned completed analysis. Enabled only by an experimental build
//! and an explicit runtime switch. No cache lock covers acquisition/projection.

#[cfg(test)]
#[path = "retention_tests.rs"]
mod tests;

use std::io::{self, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use rmcp::model::CallToolResult;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::source::{RequestControl, ReusableSource, SourceInput, SourceSnapshot};
use crate::engine::analysis_configuration::AnalysisConfiguration;
use crate::retained_memory::{self as memory, Owner, Scope};

/// Inclusive durations of existing work. Identity, source and capture are
/// inside dispatch; proof includes checks inside source and its final result
/// guard. Publish follows dispatch. Nested values must not be summed.
#[derive(Clone, Copy)]
pub(super) enum Phase {
    Identity,
    Source,
    Proof,
    Dispatch,
    Capture,
    Publish,
}

#[derive(Default)]
struct Timings {
    enabled: bool,
    elapsed: [Duration; 6],
    calls: [usize; 6],
}

thread_local! {
    static TIMINGS:std::cell::RefCell<Timings> = std::cell::RefCell::new(Timings::default());
}

pub(super) fn reset_timings(enabled: bool) {
    TIMINGS.with(|slot| {
        *slot.borrow_mut() = Timings {
            enabled,
            ..Default::default()
        }
    });
}

pub(super) struct Measurement {
    started: Option<(Phase, Instant)>,
    _thread_bound: std::marker::PhantomData<std::rc::Rc<()>>,
}

pub(super) fn measure(phase: Phase) -> Measurement {
    Measurement {
        started: TIMINGS.with(|slot| slot.borrow().enabled.then(|| (phase, Instant::now()))),
        _thread_bound: std::marker::PhantomData,
    }
}

impl Drop for Measurement {
    fn drop(&mut self) {
        if let Some((phase, started)) = self.started {
            let elapsed = started.elapsed();
            TIMINGS.with(|slot| {
                let mut timings = slot.borrow_mut();
                timings.elapsed[phase as usize] += elapsed;
                timings.calls[phase as usize] += 1;
            });
        }
    }
}

fn timings() -> Value {
    TIMINGS.with(|slot| {
        let timings = slot.borrow();
        let mut values = serde_json::Map::new();
        for (index, name) in ["identity", "source", "proof", "dispatch", "capture", "publish"].iter().enumerate() {
            values.insert((*name).into(), json!({"microseconds":timings.elapsed[index].as_secs_f64()*1_000_000.0,"calls":timings.calls[index]}));
        }
        Value::Object(values)
    })
}

pub(super) fn reset_work() {
    crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|count| count.set(0));
    crate::engine::workspace::INPUT_FILE_READS.with(|count| count.set(0));
}

fn work() -> Value {
    json!({"parse_builds":crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(std::cell::Cell::get),
        "input_checks":crate::engine::workspace::INPUT_FILE_READS.with(std::cell::Cell::get)})
}

pub(super) fn trace_disabled(trace: bool) {
    if trace {
        let _ = writeln!(
            std::io::stderr().lock(),
            "MCP_RETENTION {}",
            json!({"event":"complete","cache_enabled":false,
            "hit":false,"admitted":false,"entry_bytes":0,"retained_bytes":0,"entries":0,"work":work(),"timings":timings()})
        );
    }
}

#[derive(Clone, Copy)]
struct Limits {
    bytes: usize,
    entries: usize,
    idle: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            bytes: 128 * 1024 * 1024,
            entries: 4,
            idle: Duration::from_secs(30),
        }
    }
}

struct Entry {
    key: String,
    source: ReusableSource,
    owner: Arc<Owner>,
    used: Instant,
}
struct State {
    entries: Vec<Entry>,
    closed: bool,
    retiring: usize,
    retiring_bytes: usize,
}

struct Retirement {
    entries: Vec<Entry>,
    bytes: usize,
}

#[cfg(test)]
thread_local! {
    static RETIRE_HOOK:std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const {std::cell::RefCell::new(None)};
}

pub(super) struct Store {
    state: Mutex<State>,
    owner: Arc<Owner>,
    limits: Limits,
    trace: bool,
}

impl Store {
    fn new(limits: Limits, trace: bool) -> Arc<Self> {
        let owner = Owner::new();
        let _scope = Scope::enter(Arc::clone(&owner));
        Arc::new(Self {
            state: Mutex::new(State {
                entries: Vec::with_capacity(limits.entries),
                closed: false,
                retiring: 0,
                retiring_bytes: 0,
            }),
            owner,
            limits,
            trace,
        })
    }

    fn locked(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn charge(&self, state: &State) -> usize {
        state.entries.iter().fold(
            self.owner.bytes().saturating_add(state.retiring_bytes),
            |bytes, entry| bytes.saturating_add(entry.owner.bytes()),
        )
    }

    fn emit(&self, event: Value) {
        if self.trace {
            let _ = writeln!(std::io::stderr().lock(), "MCP_RETENTION {event}");
        }
    }

    fn take(&self, key: &str) -> Option<Entry> {
        self.expire();
        let mut state = self.locked();
        if state.closed {
            return None;
        }
        let index = state.entries.iter().position(|entry| entry.key == key)?;
        Some(state.entries.remove(index))
    }

    fn publish(&self, entry: Entry) -> bool {
        let mut retired = Vec::new();
        let retirement;
        {
            let mut state = self.locked();
            if state.closed
                || state.retiring > 0
                || !memory::installed()
                || entry.owner.bytes().saturating_add(self.owner.bytes()) > self.limits.bytes
            {
                return false;
            }
            if let Some(index) = state.entries.iter().position(|old| old.key == entry.key) {
                retired.push(state.entries.remove(index));
            }
            while !state.entries.is_empty()
                && (state.entries.len() >= self.limits.entries
                    || self.charge(&state).saturating_add(entry.owner.bytes()) > self.limits.bytes)
            {
                retired.push(state.entries.remove(0));
            }
            if retired.is_empty() {
                if state.entries.len() >= self.limits.entries {
                    return false;
                }
                state.entries.push(entry);
                return true;
            }
            retirement = Self::begin_retirement(&mut state, retired);
        }
        self.finish_retirement(retirement);
        let mut state = self.locked();
        if state.closed
            || state.retiring > 0
            || !memory::installed()
            || state.entries.len() >= self.limits.entries
            || self.charge(&state).saturating_add(entry.owner.bytes()) > self.limits.bytes
        {
            return false;
        }
        state.entries.push(entry);
        true
    }

    fn begin_retirement(state: &mut State, entries: Vec<Entry>) -> Retirement {
        let bytes = entries.iter().fold(0usize, |total, entry| {
            total.saturating_add(entry.owner.bytes())
        });
        if !entries.is_empty() {
            state.retiring += 1;
            state.retiring_bytes = state.retiring_bytes.saturating_add(bytes);
        }
        Retirement { entries, bytes }
    }

    fn finish_retirement(&self, retirement: Retirement) {
        if retirement.entries.is_empty() {
            return;
        }
        #[cfg(test)]
        RETIRE_HOOK.with(|slot| {
            let hook = slot.borrow_mut().take();
            if let Some(hook) = hook {
                hook();
            }
        });
        let bytes = retirement.bytes;
        // Reserve the old charge until its graph is destroyed, without holding
        // the lock or allowing another publisher to fill that space early.
        drop(retirement.entries);
        let mut state = self.locked();
        state.retiring -= 1;
        state.retiring_bytes = state.retiring_bytes.saturating_sub(bytes);
    }

    fn expire(&self) {
        let mut retired = Vec::new();
        let retirement;
        {
            let mut state = self.locked();
            let mut index = 0;
            while index < state.entries.len() {
                if state.entries[index].used.elapsed() >= self.limits.idle || !memory::installed() {
                    retired.push(state.entries.remove(index));
                } else {
                    index += 1;
                }
            }
            while !state.entries.is_empty() && self.charge(&state) > self.limits.bytes {
                retired.push(state.entries.remove(0));
            }
            retirement = Self::begin_retirement(&mut state, retired);
        }
        if !retirement.entries.is_empty() {
            self.finish_retirement(retirement);
            let (entries, bytes) = {
                let state = self.locked();
                (state.entries.len(), self.charge(&state))
            };
            self.emit(json!({"event":"expire","entries":entries,"retained_bytes":bytes,"budget_bytes":self.limits.bytes}));
        }
    }

    fn close(&self) {
        let retired = {
            let mut state = self.locked();
            state.closed = true;
            let entries = std::mem::take(&mut state.entries);
            Self::begin_retirement(&mut state, entries)
        };
        self.finish_retirement(retired);
        let retiring_bytes = self.locked().retiring_bytes;
        self.emit(json!({"event":"close","entries":0,"retained_bytes":retiring_bytes}));
    }
}

pub(super) struct Connection {
    store: Arc<Store>,
    reaper: tokio::task::JoinHandle<()>,
}
impl Connection {
    pub(super) fn from_env() -> Option<Self> {
        if std::env::var("DYGNOSIS_MCP_REUSE").as_deref() != Ok("1") || !memory::installed() {
            return None;
        }
        Some(Self::new(
            Limits::default(),
            std::env::var("DYGNOSIS_MCP_REUSE_TRACE").as_deref() == Ok("1"),
        ))
    }
    fn new(limits: Limits, trace: bool) -> Self {
        let store = Store::new(limits, trace);
        let _allocations = Scope::enter(Arc::clone(&store.owner));
        let weak = Arc::downgrade(&store);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let reaper = tokio::spawn(async move {
            loop {
                tick.tick().await;
                let Some(store) = weak.upgrade() else { break };
                store.expire();
            }
        });
        Self { store, reaper }
    }
    pub(super) fn store(&self) -> Arc<Store> {
        Arc::clone(&self.store)
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.store.close();
        self.reaper.abort();
    }
}

struct RequestState {
    owner: Arc<Owner>,
    used: bool,
    hit: bool,
    candidate: Option<Entry>,
}
pub(super) struct Session {
    store: Arc<Store>,
    state: Mutex<RequestState>,
}

impl Session {
    pub(super) fn new(store: &Arc<Store>) -> Arc<Self> {
        Arc::new(Self {
            store: Arc::clone(store),
            state: Mutex::new(RequestState {
                owner: Owner::new(),
                used: false,
                hit: false,
                candidate: None,
            }),
        })
    }
    fn locked(&self) -> std::sync::MutexGuard<'_, RequestState> {
        self.state.lock().unwrap_or_else(|error| error.into_inner())
    }
    pub(super) fn enter(&self) -> Scope {
        Scope::enter(Arc::clone(&self.locked().owner))
    }

    pub(super) fn checkout(
        self: &Arc<Self>,
        input: &SourceInput,
        control: &RequestControl,
        analysis: Option<&AnalysisConfiguration>,
        eligible: bool,
    ) -> Option<Lease> {
        if !eligible || !memory::installed() || control.check().is_err() {
            return None;
        }
        {
            let mut state = self.locked();
            if state.used {
                return None;
            }
            state.used = true;
        }
        let key = fingerprint(input, control, analysis)?;
        let seed = self.store.take(&key);
        if let Some(entry) = &seed {
            let owner = Arc::clone(&entry.owner);
            memory::select(Arc::clone(&owner));
            let mut state = self.locked();
            state.owner = owner;
            state.hit = true;
        }
        if self.store.trace {
            self.store
                .emit(json!({"event":"checkout","hit":seed.is_some()}));
        }
        Some(Lease {
            session: Arc::clone(self),
            key,
            seed,
        })
    }

    pub(super) fn finish(
        &self,
        result: &Result<CallToolResult, rmcp::ErrorData>,
        cancelled: &AtomicBool,
    ) {
        let (candidate, hit, owner) = {
            let mut state = self.locked();
            (state.candidate.take(), state.hit, Arc::clone(&state.owner))
        };
        let successful = result.as_ref().is_ok_and(|result| {
            result.is_error != Some(true)
                && result
                    .structured_content
                    .as_ref()
                    .is_some_and(|value| value["outcome"]["status"] != "refused")
        });
        let entry_bytes = owner.bytes();
        let admitted = {
            let _measurement = measure(Phase::Publish);
            if successful && !cancelled.load(Ordering::Relaxed) {
                candidate.is_some_and(|mut entry| {
                    entry.used = Instant::now();
                    self.store.publish(entry)
                })
            } else {
                drop(candidate);
                false
            }
        };
        if self.store.trace {
            let (entries, retained_bytes) = {
                let state = self.store.locked();
                (state.entries.len(), self.store.charge(&state))
            };
            self.store
            .emit(json!({"event":"complete","hit":hit,"admitted":admitted,
            "entry_bytes":entry_bytes,"retained_bytes":retained_bytes,"entries":entries,
            "budget_bytes":self.store.limits.bytes,"owner_peak_bytes":owner.peak_bytes(),"work":work(),"cache_enabled":true,"timings":timings()}));
        }
    }
}

pub(super) struct Lease {
    session: Arc<Session>,
    key: String,
    seed: Option<Entry>,
}
impl Lease {
    pub(super) fn source(&self) -> Option<&ReusableSource> {
        self.seed.as_ref().map(|entry| &entry.source)
    }
    pub(super) fn complete(self, snapshot: &SourceSnapshot, control: &RequestControl) {
        let _measurement = measure(Phase::Capture);
        if control.check().is_err() {
            return;
        }
        let source = snapshot.reusable();
        // The key was prepared before selecting a hit's owner. Clone it now so
        // every allocation retained by this entry belongs to the selected owner.
        let key = self.key.clone();
        let mut state = self.session.locked();
        state.candidate = Some(Entry {
            key,
            source,
            owner: Arc::clone(&state.owner),
            used: Instant::now(),
        });
    }
}

fn fingerprint(
    input: &SourceInput,
    control: &RequestControl,
    analysis: Option<&AnalysisConfiguration>,
) -> Option<String> {
    let _measurement = measure(Phase::Identity);
    if !identity_eligible(input, control, analysis) {
        return None;
    }
    #[derive(Serialize)]
    struct Identity<'a> {
        source: &'a SourceInput,
        analysis: Option<&'a AnalysisConfiguration>,
        macro_work_limit: crate::engine::macro_work::MacroWorkLimit,
        limits: &'a super::source::SourceLimits,
    }
    struct HashWriter<'a> {
        hash: Sha256,
        remaining: usize,
        control: &'a RequestControl,
    }
    impl Write for HashWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.remaining {
                return Err(io::Error::other(
                    "retention identity exceeds its work allowance",
                ));
            }
            for chunk in bytes.chunks(8192) {
                if crate::engine::control::flag_cancelled(Some(&self.control.cancelled)) {
                    return Err(io::Error::other("retention identity cancelled"));
                }
                self.hash.update(chunk);
            }
            self.remaining -= bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter {
        hash: Sha256::new(),
        remaining: control.limits.max_source_bytes,
        control,
    };
    serde_json::to_writer(
        &mut writer,
        &Identity {
            source: input,
            analysis,
            macro_work_limit: control.macro_work_limit,
            limits: &control.limits,
        },
    )
    .ok()?;
    Some(format!("{:x}", writer.hash.finalize()))
}

/// Eligibility only: the ordinary resolver still owns validation and outcomes.
/// Inspect lengths/counts before serialization can scan an oversized string.
fn identity_eligible(
    input: &SourceInput,
    control: &RequestControl,
    analysis: Option<&AnalysisConfiguration>,
) -> bool {
    use crate::engine::analysis_configuration::{
        AnalysisOverride, EquationSelection, EquationSelectionMode, GuardedTarget,
        InitialDefinition,
    };
    if control.check().is_err() {
        return false;
    }
    let limits = &control.limits;
    let items = limits.max_files.saturating_mul(limits.max_roots);
    let mut remaining = limits.max_source_bytes;
    let mut take = |text: &str| {
        let bytes = text.len().saturating_add(1);
        if control.check().is_err() || text.len() > limits.max_file_bytes || bytes > remaining {
            return false;
        }
        remaining -= bytes;
        true
    };
    let source_ok = match input {
        SourceInput::Text { text } => limits.max_files > 0 && limits.max_roots > 0 && take(text),
        SourceInput::Map {
            root,
            files,
            root_text,
            include_dirs,
            owner_roots,
        } => {
            files.len() <= limits.max_files
                && include_dirs.len() <= limits.max_files
                && owner_roots.len() <= limits.max_roots
                && take(root)
                && root_text.as_deref().is_none_or(&mut take)
                && files.iter().all(|(key, text)| take(key) && take(text))
                && include_dirs
                    .iter()
                    .chain(owner_roots)
                    .all(|path| take(path))
        }
        SourceInput::Disk {
            root,
            include_dirs,
            owner_roots,
        } => {
            include_dirs.len() <= limits.max_files
                && owner_roots.len() <= limits.max_roots
                && take(root)
                && include_dirs
                    .iter()
                    .chain(owner_roots)
                    .all(|path| take(path))
        }
    };
    if !source_ok {
        return false;
    }
    if let Some(AnalysisConfiguration {
        name,
        defines,
        overrides,
        equation_selection,
    }) = analysis
    {
        if defines.len() > items
            || overrides.len() > items
            || !name.as_deref().is_none_or(&mut take)
        {
            return false;
        }
        for InitialDefinition { name, expression } in defines {
            if !take(name) || !take(expression) {
                return false;
            }
        }
        for choice in overrides {
            let GuardedTarget {
                file,
                site_id,
                source_revision,
            } = choice.target();
            if !file.as_deref().is_none_or(&mut take) || !take(site_id) || !take(source_revision) {
                return false;
            }
            match choice {
                AnalysisOverride::Definition {
                    target: _,
                    expression,
                } => {
                    if !take(expression) {
                        return false;
                    }
                }
                AnalysisOverride::Branch {
                    target: _,
                    branch_id,
                } => {
                    if !branch_id.as_deref().is_none_or(&mut take) {
                        return false;
                    }
                }
                AnalysisOverride::SkipDefinition { target: _ } => {}
            }
        }
        if let Some(EquationSelection { mode, tag, values }) = equation_selection {
            match mode {
                EquationSelectionMode::Include | EquationSelectionMode::Exclude => {}
            }
            if values.len() > items || !take(tag) || !values.iter().all(|value| take(value)) {
                return false;
            }
        }
    }
    true
}
