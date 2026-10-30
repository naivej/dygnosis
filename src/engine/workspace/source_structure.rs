//! Written-source projection of one captured root execution. Inspection does
//! not establish execution ownership or change the effective model's coverage.

use super::*;
use crate::engine::analysis::source_inventory::SourceItemKind;
use crate::engine::analysis::source_inventory::{SourceItem, SourcePathStep};
use crate::engine::language::lexer::TokenKind;
use std::mem::size_of;
use std::sync::Weak;

const PROJECTION_CACHE_ENTRIES: usize = 32;
const PROJECTION_CACHE_BYTES: usize = 8 * 1024 * 1024;

/// Weak keys cannot keep a prior source or execution alive. Only the projected
/// payload counts against this small display cache's independent byte cap.
#[derive(Clone)]
pub(super) struct SourceProjection {
    root: String,
    file: String,
    selection: u64,
    inventory: Weak<SourceInventory>,
    trace: Option<Weak<SplicedSource>>,
    sites: Arc<[DirectiveSite]>,
    availability: Arc<[SourceItemAvailability]>,
    retained_bytes: usize,
}

impl SourceProjection {
    fn matches(
        &self,
        root: &str,
        file: &str,
        selection: u64,
        inventory: &Arc<SourceInventory>,
        trace: Option<&Arc<SplicedSource>>,
    ) -> bool {
        self.root == root
            && self.file == file
            && self.selection == selection
            && self.inventory.ptr_eq(&Arc::downgrade(inventory))
            && match (&self.trace, trace) {
                (None, None) => true,
                (Some(previous), Some(trace)) => previous.ptr_eq(&Arc::downgrade(trace)),
                _ => false,
            }
    }
}

fn optional_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map_or(0, String::capacity)
}

fn target_owned_bytes(target: &GuardedTarget) -> usize {
    optional_string_bytes(&target.file)
        .saturating_add(target.site_id.capacity())
        .saturating_add(target.source_revision.capacity())
}

type CauseKey = (u8, Span, Option<Span>);
type CauseCache = HashMap<CauseKey, Arc<AvailabilityCause>>;

fn reserve_slot<T>(
    values: &mut Vec<T>,
    budget: &mut MacroBudget,
) -> Result<(), ConfigurationError> {
    if values.len() == values.capacity() {
        let additional = values.capacity().max(1);
        spend(budget, additional.saturating_mul(size_of::<T>()))?;
        values.reserve_exact(additional);
    }
    Ok(())
}

pub(super) fn site_bytes(site: &DirectiveSite) -> usize {
    size_of::<DirectiveSite>()
        .saturating_add(target_owned_bytes(&site.target))
        .saturating_add(site.expected_text.capacity())
        .saturating_add(optional_string_bytes(&site.name))
        .saturating_add(
            site.branches
                .capacity()
                .saturating_mul(size_of::<BranchSite>()),
        )
        .saturating_add(
            site.branches
                .iter()
                .map(|branch| {
                    branch
                        .branch_id
                        .capacity()
                        .saturating_add(branch.expected_text.capacity())
                })
                .sum::<usize>(),
        )
}

fn spend(budget: &mut MacroBudget, amount: usize) -> Result<(), ConfigurationError> {
    budget.spend_work(amount.max(1)).map_err(|error| match error {
        MacroEvalError::Limit("request cancelled") => ConfigurationError::new(
            "CANCELLED", "Source inspection was cancelled; no source result was returned."),
        MacroEvalError::Limit(limit) => ConfigurationError::new(
            "SOURCE_STRUCTURE_LIMIT", format!("Source result construction stopped at the {limit} limit; no source result was returned.")),
        _ => expression_error(error),
    })
}

fn selection_revision(state: &AnalysisSelectionState, hash: &mut impl Hasher) {
    let selection = match state {
        AnalysisSelectionState::Selected(selection) => {
            false.hash(hash);
            selection
        }
        AnalysisSelectionState::Stale { selection, cause } => {
            true.hash(hash);
            cause.code.hash(hash);
            cause.message.hash(hash);
            cause.target.hash(hash);
            selection
        }
    };
    selection.configuration_id.hash(hash);
    selection.name.hash(hash);
    selection.preview.hash(hash);
}

fn guard_revision(guard: &InputGuard, hash: &mut impl Hasher) {
    guard.file.hash(hash);
    guard.exists.hash(hash);
    guard.directory.hash(hash);
    guard.identity.hash(hash);
    guard.text_sha256.hash(hash);
    guard.byte_sha256.hash(hash);
    guard.length.hash(hash);
}

/// The first excluded enclosing body is the cause. A nested condition inside
/// an excluded body was never evaluated and cannot explain that exclusion.
fn causing_step(
    item: &SourceItem,
    region: Option<&MacroAvailabilityRegion>,
) -> Option<SourcePathStep> {
    item.path
        .iter()
        .copied()
        .find(|step| region.is_none_or(|region| region.span.start <= step.branch.end))
        .or_else(|| item.path.last().copied())
}

struct CauseContext<'a> {
    file: &'a Option<String>,
    sites: &'a HashMap<Span, &'a DirectiveSite>,
    unavailable: Option<&'a str>,
}

fn availability_cause(
    state: SourceAvailability,
    item: &SourceItem,
    context: &CauseContext<'_>,
    region: Option<&MacroAvailabilityRegion>,
    cache: &mut CauseCache,
    budget: &mut MacroBudget,
    retained_bytes: &mut usize,
) -> Result<Option<Arc<AvailabilityCause>>, ConfigurationError> {
    if state == SourceAvailability::Active {
        return Ok(None);
    }
    let file = context.file;
    let sites = context.sites;
    let unavailable = context.unavailable;
    let step = causing_step(item, region);
    let site = step.and_then(|step| sites.get(&step.opener).copied());
    let branch = step.and_then(|step| {
        site?
            .branches
            .iter()
            .find(|branch| branch.directive_span == step.branch)
    });
    let span = step.map_or(item.selection, |step| {
        if step.loop_body {
            step.opener
        } else {
            step.branch
        }
    });
    let key = (state as u8, span, site.map(|site| site.directive_span));
    if let Some(cause) = cache.get(&key) {
        spend(budget, 1)?;
        return Ok(Some(Arc::clone(cause)));
    }
    let message = match state {
        SourceAvailability::Inactive if step.is_some_and(|step| step.loop_body) => {
            "The enclosing written loop body was not emitted in this occurrence."
        }
        SourceAvailability::Inactive => {
            "The enclosing written conditional arm was not emitted in this occurrence."
        }
        SourceAvailability::Mixed => {
            "Known include or loop occurrences disagree about this written region's emission; inspect its occurrences."
        }
        _ => unavailable.unwrap_or(
            "The selected root did not execute this source region, so its availability is unknown.",
        ),
    };
    let owned = message
        .len()
        .saturating_add(file.as_ref().map_or(0, String::len))
        .saturating_add(site.map_or(0, |site| target_owned_bytes(&site.target)))
        .saturating_add(branch.map_or(0, |branch| branch.branch_id.len()));
    let cause_bytes = size_of::<AvailabilityCause>()
        .saturating_add(2 * size_of::<usize>())
        .saturating_add(owned);
    // Hash-table growth is separate from the shared retained cause payload.
    if cache.len() == cache.capacity() {
        let buckets = cache
            .len()
            .saturating_add(1)
            .saturating_mul(8)
            .div_ceil(7)
            .next_power_of_two()
            .max(4);
        spend(
            budget,
            buckets
                .saturating_mul(size_of::<(CauseKey, Arc<AvailabilityCause>)>().saturating_add(1)),
        )?;
        cache.reserve(1);
    }
    spend(budget, cause_bytes)?;
    let cause = Arc::new(AvailabilityCause {
        code: match state {
            SourceAvailability::Inactive => "MACRO_EXCLUDED",
            SourceAvailability::Mixed => "MACRO_MIXED",
            _ => "MACRO_AVAILABILITY_UNKNOWN",
        },
        message: message.into(),
        file: file.clone(),
        span: Some(span),
        site: site.map(|site| site.target.clone()),
        branch_id: branch.map(|branch| branch.branch_id.clone()),
    });
    *retained_bytes = retained_bytes.saturating_add(cause_bytes);
    cache.insert(key, Arc::clone(&cause));
    Ok(Some(cause))
}

fn occurrence_id(region: &MacroAvailabilityRegion) -> String {
    use std::fmt::Write;
    let mut id = String::with_capacity(occurrence_id_len(region));
    write!(id, "include:{}", region.include_call).expect("String formatting");
    for frame in &region.loops {
        write!(id, ":loop:{}", frame.frame_id).expect("String formatting");
    }
    write!(
        id,
        ":{}:{}:{}",
        region.span.start,
        region.span.end,
        u8::from(region.active)
    )
    .expect("String formatting");
    id
}

fn occurrence_id_len(region: &MacroAvailabilityRegion) -> usize {
    fn digits(value: u64) -> usize {
        value.checked_ilog10().unwrap_or(0) as usize + 1
    }
    8usize
        .saturating_add(digits(region.include_call))
        .saturating_add(
            region
                .loops
                .iter()
                .map(|frame| 6usize.saturating_add(digits(frame.frame_id)))
                .sum::<usize>(),
        )
        .saturating_add(4)
        .saturating_add(digits(region.span.start.into()))
        .saturating_add(digits(region.span.end.into()))
}

impl Workspace {
    pub fn source_structure(
        &mut self,
        root: &str,
        query: &SourceStructureQuery,
    ) -> Result<SourceStructure, ConfigurationError> {
        let key = self.ensure_loaded(root).ok_or_else(|| {
            ConfigurationError::new("ROOT_NOT_FOUND", "The model root is unavailable.")
        })?;
        let selection_state = self.analysis_selection(root);
        let analysis_revision = if matches!(selection_state, AnalysisSelectionState::Selected(_)) {
            self.get_effective_model(root);
            self.input_revision(root)
        } else {
            None
        };
        // Refreshing disk guards can remove a changed closed target and make
        // the selected choices stale. Label and source proof use that current
        // state, not the state captured before the input refresh.
        let selection_state = self.analysis_selection(root);
        let analysis_revision = matches!(selection_state, AnalysisSelectionState::Selected(_))
            .then_some(analysis_revision)
            .flatten();
        let trace = analysis_revision
            .as_ref()
            .and_then(|_| self.spliced.get(&key))
            .cloned();
        let mut budget = self.macro_budget(&key);
        spend(&mut budget, key.len().saturating_add(query.files.len()))?;
        let mut executed = HashSet::new();
        if trace.is_some() {
            executed.insert(key.clone());
        }
        if let Some(records) = trace.as_ref().and_then(|_| self.records.get(&key)) {
            for record in &records.resolved {
                spend(&mut budget, record.path.as_os_str().len().saturating_add(1))?;
                executed.insert(self.include_key(&record.path));
            }
        }
        let mut selected = if query.files.is_empty() {
            std::iter::once(key.clone())
                .chain(executed.iter().cloned())
                .collect::<Vec<_>>()
        } else {
            let mut selected = Vec::new();
            for file in &query.files {
                let file = match file {
                    SourceFileSelection::Root => key.clone(),
                    SourceFileSelection::File(file) => {
                        spend(&mut budget, file.len())?;
                        self.include_key(&uri_to_path(file))
                    }
                };
                selected.push(file);
            }
            selected
        };
        spend(
            &mut budget,
            selected
                .len()
                .saturating_mul(selected.len().checked_ilog2().unwrap_or(0) as usize + 1),
        )?;
        selected.sort();
        selected.dedup();
        let mut revision = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut revision);
        self.overlay_only.hash(&mut revision);
        self.virtual_roots.contains(&key).hash(&mut revision);
        self.snapshot_lookup.is_some().hash(&mut revision);
        self.root_macro_work_limit(root).get().hash(&mut revision);
        self.root_paths(&key).hash(&mut revision);
        query.files.is_empty().hash(&mut revision);
        selected.hash(&mut revision);
        selection_revision(&selection_state, &mut revision);
        analysis_revision.hash(&mut revision);
        let mut selection_hash = std::collections::hash_map::DefaultHasher::new();
        match &selection_state {
            AnalysisSelectionState::Selected(selection) => {
                selection.configuration_id.hash(&mut selection_hash);
                false.hash(&mut selection_hash);
            }
            AnalysisSelectionState::Stale { selection, cause } => {
                selection.configuration_id.hash(&mut selection_hash);
                true.hash(&mut selection_hash);
                cause.code.hash(&mut selection_hash);
                cause.message.hash(&mut selection_hash);
                cause.target.hash(&mut selection_hash);
            }
        }
        let selection_stamp = selection_hash.finish();
        // Invalidating a root retains its previous execution input snapshot.
        // Stale choices have no current analysis revision: only fresh selected
        // acquisitions below can guard this written-source browsing result.
        let mut guards = if analysis_revision.is_some() {
            spend(
                &mut budget,
                self.input_snapshots.get(&key).map_or(0, |inputs| {
                    inputs
                        .iter()
                        .map(|(file, state)| {
                            file.len()
                                .saturating_add(state.bytes.as_ref().map_or(0, Vec::len))
                                .saturating_add(160)
                        })
                        .sum()
                }),
            )?;
            self.input_guards(root)
        } else {
            Vec::new()
        };
        let mut guard_files: HashSet<_> = guards.iter().map(|guard| guard.file.clone()).collect();
        let mut files = Vec::new();
        let mut pending_projections = Vec::new();
        let mut coverage = SourceCoverage {
            complete: true,
            ..Default::default()
        };
        spend(
            &mut budget,
            self.root_paths(&key)
                .iter()
                .map(|path| path.as_os_str().len())
                .sum(),
        )?;
        let search = self.root_paths(&key).to_vec();
        let mut inspected = HashSet::new();
        // Acquire only the requested/default list. Written include rows below
        // perform lookup only and cannot extend this list recursively.
        for file in selected {
            spend(&mut budget, file.len().saturating_add(1))?;
            let identity = (!self.unnamed_roots.contains(&file)).then(|| file.clone());
            let mut projection_hash = std::collections::hash_map::DefaultHasher::new();
            selection_stamp.hash(&mut projection_hash);
            identity.hash(&mut projection_hash);
            let projection_stamp = projection_hash.finish();
            let (source, mut guard) = self.source_for_inspection(&key, &file, &mut budget)?;
            let Some(_) = source else {
                let cause =
                    format!("Source file '{file}' is unavailable under this source policy.");
                guard_revision(&guard, &mut revision);
                if guard_files.insert(file.clone()) {
                    guards.push(guard);
                }
                coverage.complete = false;
                coverage.indexed_files.push(IndexedSourceCoverage {
                    file: identity,
                    source_revision: None,
                    acquired: false,
                    executed: trace.is_some() && executed.contains(&file),
                    explicitly_selected: !query.files.is_empty(),
                    complete: false,
                    cause: Some(cause),
                });
                continue;
            };
            inspected.insert(file.clone());
            let doc = Arc::clone(self.docs.get(&file).expect("acquired inspected source"));
            let inventory = doc
                .source_inventory_with_limit(
                    self.root_macro_work_limit(root),
                    self.cancellation.clone(),
                )
                .ok_or_else(|| {
                    ConfigurationError::new(
                        "CANCELLED",
                        "Source inspection was cancelled; no source result was returned.",
                    )
                })?;
            let macro_source = doc.macro_source(&mut budget).ok();
            let source_revision = macro_source.as_ref().map_or_else(
                || inventory.source_revision.clone(),
                |source| source.source_revision.clone(),
            );
            if !source_revision.is_empty() {
                guard.text_sha256 = Some(source_revision.clone());
                if guard.byte_sha256.is_none() {
                    guard.byte_sha256 = Some(source_revision.clone());
                }
            }
            guard_revision(&guard, &mut revision);
            if guard_files.insert(file.clone()) {
                guards.push(guard);
            }
            spend(
                &mut budget,
                source_revision.len().saturating_add(inventory.items.len()),
            )?;
            source_revision.hash(&mut revision);
            let cause = inventory
                .structure_cause()
                .map(str::to_owned)
                .or_else(|| inventory.target_coverage_cause.clone())
                .or_else(|| {
                    trace
                        .as_ref()
                        .and_then(|trace| trace.availability_cause.clone())
                });
            let complete = inventory.structure_complete()
                && inventory.targets_complete()
                && trace
                    .as_ref()
                    .is_none_or(|trace| trace.availability_cause.is_none());
            coverage.complete &= complete;
            coverage.indexed_files.push(IndexedSourceCoverage {
                file: identity.clone(),
                source_revision: Some(source_revision.clone()),
                acquired: true,
                executed: trace.is_some() && executed.contains(&file),
                explicitly_selected: !query.files.is_empty(),
                complete,
                cause,
            });
            if let Some(cached) = self.source_projections.iter().find(|cached| {
                cached.matches(&key, &file, projection_stamp, &inventory, trace.as_ref())
            }) {
                spend(&mut budget, 2)?;
                files.push(SourceFileStructure {
                    lookup: None,
                    file: identity,
                    source_revision,
                    text: Arc::clone(&doc.source),
                    inventory,
                    sites: Arc::clone(&cached.sites),
                    availability: Arc::clone(&cached.availability),
                });
                continue;
            }
            // Count retained shallow rows, Vec capacities and owned strings as
            // they are constructed. Arc headers and cache keys count too; work
            // units remain a separate construction bound.
            let mut retained_bytes = size_of::<SourceProjection>()
                .saturating_add(key.len())
                .saturating_add(file.len())
                .saturating_add(4 * size_of::<usize>());
            spend(
                &mut budget,
                retained_bytes
                    .saturating_add(
                        inventory
                            .sites
                            .len()
                            .saturating_mul(size_of::<DirectiveSite>()),
                    )
                    .saturating_add(
                        inventory
                            .items
                            .len()
                            .saturating_mul(size_of::<SourceItemAvailability>()),
                    ),
            )?;
            let mut sites = Vec::with_capacity(inventory.sites.len());
            for site in inventory.sites.iter() {
                let size = site
                    .expected_text
                    .len()
                    .saturating_add(site.target.site_id.len())
                    .saturating_add(site.target.source_revision.len())
                    .saturating_add(file.len())
                    .saturating_add(site.name.as_ref().map_or(0, String::len))
                    .saturating_add(
                        site.branches
                            .iter()
                            .map(|branch| {
                                branch
                                    .expected_text
                                    .len()
                                    .saturating_add(branch.branch_id.len())
                                    .saturating_add(size_of::<BranchSite>())
                            })
                            .sum::<usize>(),
                    );
                spend(&mut budget, size)?;
                let mut bound = site.clone();
                bound.target.file = identity.clone();
                retained_bytes = retained_bytes.saturating_add(site_bytes(&bound));
                sites.push(bound);
            }
            spend(
                &mut budget,
                sites.len().saturating_mul(
                    size_of::<(Span, &DirectiveSite)>()
                        .saturating_mul(2)
                        .saturating_add(2),
                ),
            )?;
            let sites_by_span: HashMap<_, _> = sites
                .iter()
                .map(|site| (site.directive_span, site))
                .collect();
            let proven = trace
                .as_ref()
                .is_some_and(|trace| trace.macro_complete && trace.availability_cause.is_none());
            let unavailable = match &selection_state {
                AnalysisSelectionState::Stale { cause, .. } => Some(cause.message.as_str()),
                _ => trace.as_ref().and_then(|trace| {
                    trace.availability_cause.as_deref()
                        .or_else(|| trace.limit.as_ref().map(|limit| limit.message.as_str()))
                        .or_else(|| trace.refusal.as_ref().map(|(_, _, _, message)| message.as_str()))
                        .or_else(|| self.records.get(&key).filter(|records| !records.unresolved.is_empty()).map(|_| "An executed include could not be acquired; reached occurrences are retained, but complete availability is unknown."))
                        .or_else(|| (!trace.macro_complete).then_some("Macro execution stopped before completion; reached occurrences are retained, but complete availability is unknown."))
                }),
            };
            let cause_context = CauseContext {
                file: &identity,
                sites: &sites_by_span,
                unavailable,
            };
            let mut availability = Vec::with_capacity(inventory.items.len());
            let mut causes = CauseCache::new();
            for item in &inventory.items {
                spend(
                    &mut budget,
                    item.source_id
                        .len()
                        .saturating_add(item.path.len())
                        .saturating_add(1),
                )?;
                let state = trace.as_ref().map_or(SourceAvailability::Unknown, |trace| {
                    trace
                        .availability_index
                        .summary(&file, item.selection, proven)
                });
                let mut occurrences = Vec::new();
                let (mut control_active, mut control_inactive) = (false, false);
                let mut occurrence_owned = 0usize;
                let mut excluded = None;
                if let Some(trace) = trace.as_ref() {
                    // The execution owner publishes each token/directive or
                    // excluded body once per call/iteration. Its occurrence
                    // identity is unique; projection need not clone IDs into
                    // another per-row de-duplication table.
                    for region in trace.availability_index.matching(&file, item.selection) {
                        if region.control_active.unwrap_or(region.active) {
                            control_active = true;
                        } else {
                            control_inactive = true;
                        }
                        let loop_strings = region
                            .loops
                            .iter()
                            .map(|frame| {
                                frame
                                    .variable
                                    .as_ref()
                                    .map_or(0, String::len)
                                    .saturating_add(frame.value.as_ref().map_or(0, String::len))
                            })
                            .sum::<usize>();
                        spend(
                            &mut budget,
                            loop_strings
                                .saturating_add(
                                    region
                                        .loops
                                        .len()
                                        .saturating_mul(size_of::<SourceLoopOccurrence>()),
                                )
                                .saturating_add(occurrence_id_len(region)),
                        )?;
                        reserve_slot(&mut occurrences, &mut budget)?;
                        let id = occurrence_id(region);
                        let emitted = if region.active {
                            SourceAvailability::Active
                        } else {
                            SourceAvailability::Inactive
                        };
                        if !region.active && excluded.is_none() {
                            excluded = Some(region);
                        }
                        let mut loops = Vec::with_capacity(region.loops.len());
                        let mut loop_string_capacity = 0usize;
                        for frame in &region.loops {
                            let copied = frame.clone();
                            loop_string_capacity = loop_string_capacity
                                .saturating_add(optional_string_bytes(&copied.variable))
                                .saturating_add(optional_string_bytes(&copied.value));
                            loops.push(copied);
                        }
                        let occurrence = SourceItemOccurrence {
                            occurrence_id: id,
                            include_call: region.include_call,
                            parent_include_call: region.parent_include_call,
                            loops,
                            availability: emitted,
                            cause: availability_cause(
                                emitted,
                                item,
                                &cause_context,
                                Some(region),
                                &mut causes,
                                &mut budget,
                                &mut retained_bytes,
                            )?,
                        };
                        occurrence_owned = occurrence_owned
                            .saturating_add(occurrence.occurrence_id.capacity())
                            .saturating_add(
                                occurrence
                                    .loops
                                    .capacity()
                                    .saturating_mul(size_of::<SourceLoopOccurrence>()),
                            )
                            .saturating_add(loop_string_capacity);
                        occurrences.push(occurrence);
                    }
                }
                let cause = availability_cause(
                    state,
                    item,
                    &cause_context,
                    excluded,
                    &mut causes,
                    &mut budget,
                    &mut retained_bytes,
                )?;
                let control_availability = if !proven {
                    SourceAvailability::Unknown
                } else {
                    match (control_active, control_inactive) {
                        (true, true) => SourceAvailability::Mixed,
                        (true, false) => SourceAvailability::Active,
                        (false, true) => SourceAvailability::Inactive,
                        _ => SourceAvailability::Unknown,
                    }
                };
                let control_cause = if control_availability == SourceAvailability::Unknown
                    && state != control_availability
                    && matches!(
                        item.kind,
                        SourceItemKind::Conditional | SourceItemKind::Branch
                    ) {
                    availability_cause(
                        control_availability,
                        item,
                        &cause_context,
                        excluded,
                        &mut causes,
                        &mut budget,
                        &mut retained_bytes,
                    )?
                } else {
                    cause.clone()
                };
                let projected = SourceItemAvailability {
                    source_id: item.source_id.clone(),
                    availability: state,
                    control_availability,
                    control_cause,
                    cause,
                    occurrences,
                };
                retained_bytes = retained_bytes
                    .saturating_add(size_of::<SourceItemAvailability>())
                    .saturating_add(projected.source_id.capacity())
                    .saturating_add(
                        projected
                            .occurrences
                            .capacity()
                            .saturating_mul(size_of::<SourceItemOccurrence>()),
                    )
                    .saturating_add(occurrence_owned);
                availability.push(projected);
            }
            let sites: Arc<[DirectiveSite]> = sites.into();
            let availability: Arc<[SourceItemAvailability]> = availability.into();
            if retained_bytes <= PROJECTION_CACHE_BYTES {
                pending_projections.push(SourceProjection {
                    root: key.clone(),
                    file: file.clone(),
                    selection: projection_stamp,
                    inventory: Arc::downgrade(&inventory),
                    trace: trace.as_ref().map(Arc::downgrade),
                    sites: Arc::clone(&sites),
                    availability: Arc::clone(&availability),
                    retained_bytes,
                });
            }
            files.push(SourceFileStructure {
                lookup: None,
                file: identity,
                source_revision,
                text: Arc::clone(&doc.source),
                inventory,
                sites,
                availability,
            });
        }
        // Include targets are projected only after the explicit list has been
        // acquired, so selecting a dormant literal target can satisfy browsing
        // coverage without making it an executed source.
        let lookup = Arc::new(self.source_lookup(&mut budget).map_err(expression_error)?);
        for file in &mut files {
            file.lookup = Some(Arc::clone(&lookup));
        }
        let mut unresolved = HashMap::new();
        if let Some(records) = trace.as_ref().and_then(|_| self.records.get(&key)) {
            for missing in &records.unresolved {
                spend(
                    &mut budget,
                    missing
                        .site
                        .file
                        .len()
                        .saturating_add(missing.filename.len())
                        .saturating_add(missing.searched.iter().map(String::len).sum::<usize>())
                        .saturating_add(8),
                )?;
                unresolved.insert(
                    (missing.site.file.clone(), missing.site.span),
                    (missing.filename.clone(), missing.searched.clone()),
                );
            }
        }
        for file in &files {
            let file_key = file.file.as_deref().unwrap_or(&key);
            let source = self
                .docs
                .get(file_key)
                .expect("retained inspected source")
                .macro_source(&mut budget)
                .ok();
            let Some(source) = source else {
                continue;
            };
            for token in source.tokens.iter() {
                spend(&mut budget, 1)?;
                if token.kind != TokenKind::MacroDir
                    || !crate::engine::language::macro_expand::directive_name(
                        token.text(&source.normalized),
                    )
                    .eq_ignore_ascii_case("include")
                {
                    continue;
                }
                let expression = token.text(&source.original);
                spend(
                    &mut budget,
                    expression
                        .len()
                        .saturating_add(file_key.len())
                        .saturating_add(
                            search
                                .iter()
                                .map(|path| path.as_os_str().len())
                                .sum::<usize>(),
                        )
                        .saturating_add(192),
                )?;
                let literal = crate::engine::language::parser::literal_include_filename(expression);
                let failed = unresolved.get(&(file_key.to_owned(), token.span));
                let emitted = trace.as_ref().is_some_and(|trace| {
                    trace
                        .availability_index
                        .matching(file_key, token.span)
                        .any(|region| region.active)
                });
                if failed.is_none()
                    && trace.as_ref().is_some_and(|trace| {
                        trace
                            .acquired_include_sites
                            .contains(&(file_key.to_owned(), token.span))
                    })
                {
                    continue;
                }
                let target = if failed.is_none() {
                    literal.as_deref().and_then(|name| {
                        self.resolve_source_target(&key, file_key, name, &search, &lookup)
                    })
                } else {
                    None
                };
                if target
                    .as_ref()
                    .is_some_and(|target| inspected.contains(target))
                    && !emitted
                {
                    continue;
                }
                let cause = if failed.is_some() || emitted {
                    "This executed include target was not proven acquired; the active analysis is incomplete."
                } else if literal.is_none() {
                    "This computed include target has no proven value for source inspection."
                } else if target.is_some() {
                    "This written literal include target is available for Open Source but has not been acquired for source inspection."
                } else {
                    "This written literal include target is unavailable in the current source lookup context."
                };
                let lookup_context = failed.map_or_else(
                    || {
                        search
                            .iter()
                            .map(|path| path.to_string_lossy().into_owned())
                            .collect()
                    },
                    |(_, searched)| searched.clone(),
                );
                coverage.unresolved_targets.push(UnresolvedSourceTarget {
                    file: file.file.clone(),
                    span: token.span,
                    expression: expression.to_owned(),
                    literal_target: literal,
                    cause: cause.into(),
                    lookup_context,
                });
            }
        }
        coverage.complete &= coverage.unresolved_targets.is_empty();
        for indexed in &coverage.indexed_files {
            indexed.file.hash(&mut revision);
            indexed.source_revision.hash(&mut revision);
            indexed.acquired.hash(&mut revision);
            indexed.executed.hash(&mut revision);
            indexed.explicitly_selected.hash(&mut revision);
            indexed.complete.hash(&mut revision);
            indexed.cause.hash(&mut revision);
        }
        for missing in &coverage.unresolved_targets {
            missing.file.hash(&mut revision);
            missing.span.hash(&mut revision);
            missing.expression.hash(&mut revision);
            missing.literal_target.hash(&mut revision);
            missing.cause.hash(&mut revision);
            missing.lookup_context.hash(&mut revision);
        }
        coverage.complete.hash(&mut revision);
        for guard in &guards {
            guard_revision(guard, &mut revision);
        }
        spend(&mut budget, 1)?;
        // Publish only after the complete request passed cancellation and work
        // checks. A refused request cannot leave partially built cache entries.
        self.source_projections.retain(|entry| {
            entry.inventory.strong_count() > 0
                && entry
                    .trace
                    .as_ref()
                    .is_none_or(|trace| trace.strong_count() > 0)
        });
        for projection in pending_projections {
            while !self.source_projections.is_empty()
                && (self.source_projections.len() >= PROJECTION_CACHE_ENTRIES
                    || self
                        .source_projections
                        .iter()
                        .map(|entry| entry.retained_bytes)
                        .sum::<usize>()
                        .saturating_add(projection.retained_bytes)
                        > PROJECTION_CACHE_BYTES)
            {
                self.source_projections.remove(0);
            }
            self.source_projections.push(projection);
        }
        Ok(SourceStructure {
            selection_state,
            revision: format!("{:016x}", revision.finish()),
            analysis_revision,
            files,
            coverage,
            input_guards: guards,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(source: &str, other: &[(&str, &str)]) -> Workspace {
        let mut files = BTreeMap::from([("main.mod".to_owned(), source.to_owned())]);
        files.extend(
            other
                .iter()
                .map(|(file, text)| (file.to_string(), text.to_string())),
        );
        Workspace::overlay_documents(&files)
    }

    fn named<'a>(file: &'a SourceFileStructure, name: &str) -> &'a SourceItemAvailability {
        let index = file
            .inventory
            .items
            .iter()
            .position(|item| item.name.as_deref() == Some(name))
            .expect("written named row");
        &file.availability[index]
    }

    #[test]
    fn excluded_nested_source_names_the_evaluated_parent_guard() {
        // A never-evaluated nested condition cannot be the cause of exclusion.
        let mut ws = workspace(
            "@#if 0\n@#if MISSING\nvar hidden also_hidden;\n@#endif\n@#else\nvar live;\n@#endif\n",
            &[],
        );
        let view = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        let file = &view.files[0];
        let hidden = named(file, "hidden");
        assert_eq!(hidden.availability, SourceAvailability::Inactive);
        let cause = hidden.cause.as_ref().unwrap();
        let outer = file
            .sites
            .iter()
            .find(|site| site.expected_text == "@#if 0")
            .unwrap();
        assert_eq!(cause.site.as_ref(), Some(&outer.target));
        assert_eq!(
            cause.branch_id.as_deref(),
            Some(outer.branches[0].branch_id.as_str())
        );
        assert!(Arc::ptr_eq(
            hidden.cause.as_ref().unwrap(),
            named(file, "also_hidden").cause.as_ref().unwrap()
        ));
        assert_eq!(named(file, "live").availability, SourceAvailability::Active);
    }

    #[test]
    fn included_mixed_occurrences_keep_caller_tuple_values_and_stable_ids() {
        // Covers collapsing include visits, losing caller tuple context, and
        // assigning occurrence IDs from response/query order.
        let mut ws = workspace(
            "@#for (i,j) in [(1,2),(2,3)]\n@#include \"part.inc\"\n@#endfor\n",
            &[("part.inc", "@#if i == 1\nvar selected;\n@#endif\n")],
        );
        let view = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        let file = view
            .files
            .iter()
            .find(|file| file.file.as_deref() == Some("part.inc"))
            .unwrap();
        let selected = named(file, "selected");
        assert_eq!(selected.availability, SourceAvailability::Mixed);
        assert!(
            selected
                .occurrences
                .iter()
                .any(|occurrence| occurrence.availability == SourceAvailability::Active)
        );
        assert!(
            selected
                .occurrences
                .iter()
                .any(|occurrence| occurrence.availability == SourceAvailability::Inactive)
        );
        let calls: HashSet<_> = selected
            .occurrences
            .iter()
            .map(|occurrence| occurrence.include_call)
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(selected.occurrences.iter().all(|occurrence| {
            occurrence.parent_include_call.is_some()
                && occurrence.loops.iter().any(|frame| {
                    frame.variable.as_deref() == Some("(i,j)") && frame.value.is_some()
                })
        }));
        let ids: HashSet<_> = selected
            .occurrences
            .iter()
            .map(|occurrence| occurrence.occurrence_id.clone())
            .collect();
        let filtered = ws
            .source_structure(
                "main.mod",
                &SourceStructureQuery {
                    files: vec![SourceFileSelection::File("part.inc".into())],
                },
            )
            .unwrap();
        assert_eq!(
            ids,
            named(&filtered.files[0], "selected")
                .occurrences
                .iter()
                .map(|occurrence| occurrence.occurrence_id.clone())
                .collect()
        );
        assert!(Arc::ptr_eq(&file.inventory, &filtered.files[0].inventory));
        assert!(Arc::ptr_eq(&file.sites, &filtered.files[0].sites));
        assert!(Arc::ptr_eq(
            &file.availability,
            &filtered.files[0].availability
        ));
    }

    #[test]
    fn source_inspection_satisfies_dormant_literal_without_execution_ownership() {
        // Inspecting a literal dormant target satisfies browsing coverage but
        // cannot add an executed include, declaration, or semantic owner.
        let mut ws = workspace(
            "@#if 0\n@#include \"dormant.inc\"\n@#endif\nvar live;\n",
            &[
                ("dormant.inc", "var dormant;\n"),
                ("arbitrary.inc", "parameters unrelated;\n"),
            ],
        );
        let initial = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert_eq!(initial.files.len(), 1);
        assert!(
            initial
                .coverage
                .unresolved_targets
                .iter()
                .any(|target| target.literal_target.as_deref() == Some("dormant.inc"))
        );
        let analysis_revision = initial.analysis_revision.clone();
        let query = SourceStructureQuery {
            files: vec![
                SourceFileSelection::Root,
                SourceFileSelection::File("dormant.inc".into()),
                SourceFileSelection::File("arbitrary.inc".into()),
            ],
        };
        let inspected = ws.source_structure("main.mod", &query).unwrap();
        assert!(inspected.coverage.unresolved_targets.is_empty());
        assert_eq!(inspected.analysis_revision, analysis_revision);
        for file in ["dormant.inc", "arbitrary.inc"] {
            let coverage = inspected
                .coverage
                .indexed_files
                .iter()
                .find(|item| item.file.as_deref() == Some(file))
                .unwrap();
            assert!(coverage.acquired && coverage.explicitly_selected && !coverage.executed);
            assert!(ws.owner_roots(file).is_empty());
            let guard = inspected
                .input_guards
                .iter()
                .find(|guard| guard.file == file)
                .unwrap();
            assert!(
                guard.exists
                    && !guard.directory
                    && guard.text_sha256.is_some()
                    && guard.byte_sha256.is_some()
            );
        }
        assert_eq!(
            ws.get_effective_model("main.mod").unwrap().endogenous.len(),
            1
        );
        assert!(ws.include_records("main.mod").unwrap().resolved.is_empty());
    }

    #[test]
    fn active_missing_include_stays_unresolved_and_unknown_beats_known_prefix() {
        // A visited missing include cannot vanish from source coverage. The
        // reached branch remains an occurrence while aggregate proof is unknown.
        let mut ws = workspace(
            "@#if 1\nvar reached;\n@#endif\n@#include \"missing.inc\"\nvar later;\n",
            &[],
        );
        let view = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        let missing = view
            .coverage
            .unresolved_targets
            .iter()
            .find(|item| item.literal_target.as_deref() == Some("missing.inc"))
            .unwrap();
        assert!(missing.cause.contains("executed include"));
        assert!(!view.coverage.complete);
        let reached = named(&view.files[0], "reached");
        assert_eq!(reached.availability, SourceAvailability::Unknown);
        assert!(
            reached
                .occurrences
                .iter()
                .any(|occurrence| occurrence.availability == SourceAvailability::Active)
        );
        assert_eq!(
            named(&view.files[0], "later").availability,
            SourceAvailability::Unknown
        );
    }

    #[test]
    fn stale_choices_and_inspected_bytes_guard_the_source_revision() {
        // A source-only revision must change for inspected text and stale
        // configuration choices even when no effective analysis is available.
        let source = "@#define FLAG = 0\n@#if FLAG\nvar a;\n@#else\nvar b;\n@#endif\n";
        let mut ws = workspace(source, &[("other.inc", "var first;\n")]);
        let initial = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        let target = initial.files[0]
            .sites
            .iter()
            .find(|site| site.expected_text == "@#define FLAG = 0")
            .unwrap()
            .target
            .clone();
        let choices = |expression: &str| AnalysisConfiguration {
            overrides: vec![AnalysisOverride::Definition {
                target: target.clone(),
                expression: expression.into(),
            }],
            ..Default::default()
        };
        let query = SourceStructureQuery {
            files: vec![
                SourceFileSelection::Root,
                SourceFileSelection::File("other.inc".into()),
            ],
        };
        ws.set_root_analysis_configuration("main.mod", choices("1"))
            .unwrap();
        ws.update_document("main.mod", format!("{source}// changed\n"));
        let stale = ws.source_structure("main.mod", &query).unwrap();
        assert!(matches!(
            stale.selection_state,
            AnalysisSelectionState::Stale { .. }
        ));
        assert!(stale.analysis_revision.is_none());
        assert!(
            stale
                .coverage
                .indexed_files
                .iter()
                .all(|file| !file.executed)
        );
        for file in &stale.files {
            let guard = stale
                .input_guards
                .iter()
                .find(|guard| Some(guard.file.as_str()) == file.file.as_deref())
                .unwrap();
            let current =
                crate::engine::analysis_configuration::source_revision(file.text.as_str());
            assert_eq!(guard.text_sha256.as_deref(), Some(current.as_str()));
            assert_eq!(guard.byte_sha256.as_deref(), Some(current.as_str()));
        }
        let default_stale = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(
            default_stale
                .files
                .iter()
                .any(|file| file.file.as_deref() == Some("main.mod"))
        );
        assert!(
            default_stale
                .coverage
                .indexed_files
                .iter()
                .all(|file| !file.executed)
        );
        assert!(
            stale
                .files
                .iter()
                .flat_map(|file| file.availability.iter())
                .all(|row| row.availability == SourceAvailability::Unknown)
        );
        ws.update_document("other.inc", "var second;\n");
        let changed = ws.source_structure("main.mod", &query).unwrap();
        assert_ne!(stale.revision, changed.revision);
        let other_guard = changed
            .input_guards
            .iter()
            .find(|guard| guard.file == "other.inc")
            .unwrap();
        assert_eq!(
            other_guard.text_sha256.as_deref(),
            Some(crate::engine::analysis_configuration::source_revision("var second;\n").as_str())
        );
        let mut other = workspace(source, &[("other.inc", "var second;\n")]);
        other
            .set_root_analysis_configuration("main.mod", choices("2"))
            .unwrap();
        other.update_document("main.mod", format!("{source}// changed\n"));
        let other = other.source_structure("main.mod", &query).unwrap();
        assert_ne!(changed.revision, other.revision);
    }

    #[test]
    fn disk_guard_refresh_marks_changed_closed_override_targets_stale() {
        // Guards publishing Selected from cached documents after input_revision
        // finds a changed closed override target, including root-only browsing.
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root_path = std::env::temp_dir().join(format!("dygnosis-source-stale-{unique}.mod"));
        let target_path = std::env::temp_dir().join(format!("dygnosis-source-target-{unique}.inc"));
        let source = format!(
            "@#include \"{}\"\n@#if FLAG\nvar selected;\n@#else\nvar original;\n@#endif\n",
            target_path.file_name().unwrap().to_str().unwrap()
        );
        std::fs::write(&root_path, &source).unwrap();
        let root = root_path.to_str().unwrap();
        for query in [
            SourceStructureQuery {
                files: vec![SourceFileSelection::Root],
            },
            SourceStructureQuery::default(),
        ] {
            std::fs::write(&target_path, "@#define FLAG=0\n").unwrap();
            let mut ws = Workspace::new();
            ws.load_from_disk(&root_path).unwrap();
            let initial = ws
                .source_structure(root, &SourceStructureQuery::default())
                .unwrap();
            let target = initial
                .files
                .iter()
                .flat_map(|file| file.sites.iter())
                .find(|site| site.expected_text == "@#define FLAG=0")
                .unwrap()
                .target
                .clone();
            let selection = ws
                .set_root_analysis_configuration(
                    root,
                    AnalysisConfiguration {
                        overrides: vec![AnalysisOverride::Definition {
                            target: target.clone(),
                            expression: "1".into(),
                        }],
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(ws.input_revision(root).is_some());
            std::fs::write(&target_path, "@#define FLAG=2\n").unwrap();
            let view = ws.source_structure(root, &query).unwrap();
            let AnalysisSelectionState::Stale {
                selection: current,
                cause,
            } = &view.selection_state
            else {
                panic!("changed closed target retained a Selected source label");
            };
            assert_eq!(current.configuration_id, selection.configuration_id);
            assert_eq!(cause.target.as_ref(), Some(&target));
            assert!(view.analysis_revision.is_none());
            assert!(
                view.files
                    .iter()
                    .all(|file| file.file.as_deref() == Some(normalize_uri(root).as_str()))
            );
            assert!(
                view.files
                    .iter()
                    .flat_map(|file| file.availability.iter())
                    .all(|item| item.availability == SourceAvailability::Unknown
                        && item.occurrences.is_empty())
            );
            assert!(
                view.coverage
                    .indexed_files
                    .iter()
                    .all(|file| !file.executed)
            );
            assert!(
                view.files
                    .iter()
                    .flat_map(|file| file.availability.iter())
                    .filter_map(|item| item.cause.as_ref())
                    .all(|item| item.message == cause.message)
            );
            assert!(ws.get_effective_model(root).is_none());
        }
        std::fs::remove_file(target_path).unwrap();
        std::fs::remove_file(root_path).unwrap();
    }

    #[test]
    fn source_projection_cache_reuses_rows_and_invalidates_configuration_and_text() {
        // Guards repeated display allocation and reuse across a different
        // execution or inventory. Acquisition coverage remains independently fresh.
        let source = "@#ifdef FLAG\nvar selected;\n@#else\nvar source;\n@#endif\n";
        let mut ws = workspace(source, &[]);
        let first = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        let warm = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(Arc::ptr_eq(&first.files[0].sites, &warm.files[0].sites));
        assert!(Arc::ptr_eq(
            &first.files[0].availability,
            &warm.files[0].availability
        ));
        ws.set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                defines: vec![InitialDefinition {
                    name: "FLAG".into(),
                    expression: "1".into(),
                }],
                ..Default::default()
            },
        )
        .unwrap();
        let configured = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(!Arc::ptr_eq(
            &warm.files[0].availability,
            &configured.files[0].availability
        ));
        assert_eq!(
            named(&configured.files[0], "selected").availability,
            SourceAvailability::Active
        );
        ws.update_document("main.mod", format!("{source}// edit\n"));
        let edited = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(!Arc::ptr_eq(
            &configured.files[0].sites,
            &edited.files[0].sites
        ));
        assert!(ws.source_projections.len() <= PROJECTION_CACHE_ENTRIES);
        assert!(
            ws.source_projections
                .iter()
                .map(|entry| entry.retained_bytes)
                .sum::<usize>()
                <= PROJECTION_CACHE_BYTES
        );
    }

    #[test]
    fn multi_file_source_query_reuses_projections_after_label_only_change() {
        // Guards per-file cache thrashing on ordinary include roots, and
        // label-only changes rebuilding display rows with unchanged facts.
        let mut ws = workspace(
            "@#include \"one.inc\"\n@#include \"two.inc\"\n@#include \"three.inc\"\n",
            &[
                ("one.inc", "var a;\n"),
                ("two.inc", "var b;\n"),
                ("three.inc", "var c;\n"),
            ],
        );
        let first = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(first.files.len() > 2);
        ws.set_root_analysis_configuration(
            "main.mod",
            AnalysisConfiguration {
                name: Some("Named source".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let warm = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert_ne!(first.revision, warm.revision);
        assert!(first.files.iter().all(|before| {
            warm.files
                .iter()
                .find(|after| before.file == after.file)
                .is_some_and(|after| {
                    Arc::ptr_eq(&before.sites, &after.sites)
                        && Arc::ptr_eq(&before.availability, &after.availability)
                })
        }));
    }

    #[test]
    fn thousand_equation_source_projection_stays_within_the_default_work_limit() {
        // Guards refusing an ordinary large source solely because every row
        // paid a fixed worst-case copy charge, or copied each repeated cause.
        let declarations = format!(
            "var {};\n",
            (0..1_000)
                .map(|i| format!("y{i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let equations = (0..1_000)
            .map(|i| format!("y{i}={i};\n"))
            .collect::<String>();
        let defaults = "@#ifndef MODE\n@#define MODE=1\n@#endif\n";
        for shape in ["plain", "nested", "shared"] {
            let source = match shape {
                "plain" => format!("{declarations}model;\n{equations}end;\n"),
                "nested" => format!(
                    "{defaults}{declarations}model;\n@#if MODE==1\n{equations}@#else\n@#if MODE==2\n{equations}@#endif\n@#endif\nend;\n"
                ),
                _ => format!(
                    "{defaults}{declarations}model;\n@#for copy in 1:4\n@#include \"equations.inc\"\n@#endfor\nend;\n"
                ),
            };
            let helper = format!("@#if MODE==copy\n{equations}@#endif\n");
            let supplied_helper = [("equations.inc", helper.as_str())];
            let mut ws = workspace(
                &source,
                if shape == "shared" {
                    &supplied_helper
                } else {
                    &[]
                },
            );
            assert!(ws.get_effective_model("main.mod").is_some());
            let view = ws
                .source_structure("main.mod", &SourceStructureQuery::default())
                .unwrap_or_else(|error| panic!("{shape}: {} {}", error.code, error.message));
            assert!(view.coverage.complete, "{shape}: source coverage stopped");
            assert!(view.files.iter().any(|file| {
                file.inventory
                    .items
                    .iter()
                    .any(|item| item.name.as_deref() == Some("y999"))
            }));
            let rows: usize = view.files.iter().map(|file| file.availability.len()).sum();
            let occurrences: usize = view
                .files
                .iter()
                .flat_map(|file| file.availability.iter())
                .map(|row| row.occurrences.len())
                .sum();
            eprintln!(
                "source projection {shape}: {rows} written rows, {occurrences} occurrences, {} retained bytes",
                ws.source_projections
                    .iter()
                    .map(|projection| projection.retained_bytes)
                    .sum::<usize>()
            );
            let warm = ws
                .source_structure("main.mod", &SourceStructureQuery::default())
                .unwrap();
            assert!(view.files.iter().all(|before| {
                warm.files
                    .iter()
                    .find(|after| before.file == after.file)
                    .is_some_and(|after| Arc::ptr_eq(&before.availability, &after.availability))
            }));
        }
    }

    #[test]
    fn cancelled_source_request_does_not_publish_projection_entries() {
        // Guards a refused request retaining a new partial display result.
        let mut ws = workspace("var y;\n", &[]);
        ws.set_cancellation(Arc::new(AtomicBool::new(true)));
        assert!(
            ws.source_structure("main.mod", &SourceStructureQuery::default())
                .is_err()
        );
        assert!(ws.source_projections.is_empty());
    }

    #[test]
    fn successful_computed_include_remains_acquired_before_later_failure() {
        // A later macro failure cannot erase a known acquired computed include.
        let mut ws = workspace(
            "@#define FILE = \"part.inc\"\n@#include FILE\n@#define BAD = MISSING\n",
            &[("part.inc", "var reached;\n")],
        );
        let view = ws
            .source_structure("main.mod", &SourceStructureQuery::default())
            .unwrap();
        assert!(view.coverage.unresolved_targets.is_empty());
        assert!(
            view.coverage
                .indexed_files
                .iter()
                .any(|file| file.file.as_deref() == Some("part.inc")
                    && file.acquired
                    && file.executed)
        );
        let part = view
            .files
            .iter()
            .find(|file| file.file.as_deref() == Some("part.inc"))
            .unwrap();
        assert_eq!(
            named(part, "reached").availability,
            SourceAvailability::Unknown
        );
        assert!(
            named(part, "reached")
                .occurrences
                .iter()
                .any(|occurrence| occurrence.availability == SourceAvailability::Active)
        );
    }

    #[test]
    fn missing_explicit_source_has_negative_guard_and_never_uses_disk_in_map_mode() {
        // A supplied-only request cannot acquire a host file through an
        // explicit selection or quietly treat an unavailable file as indexed.
        let mut ws = workspace("var live;\n", &[]);
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("Cargo.toml")
            .to_string_lossy()
            .into_owned();
        let query = SourceStructureQuery {
            files: vec![
                SourceFileSelection::Root,
                SourceFileSelection::File(path.clone()),
            ],
        };
        let view = ws.source_structure("main.mod", &query).unwrap();
        assert_eq!(view.files.len(), 1);
        let missing = view
            .coverage
            .indexed_files
            .iter()
            .find(|file| !file.acquired)
            .unwrap();
        assert!(missing.explicitly_selected && !missing.complete && !missing.executed);
        assert!(!view.coverage.complete);
        assert!(
            view.input_guards
                .iter()
                .any(|guard| !guard.exists && guard.file == missing.file.as_deref().unwrap())
        );
    }
}
