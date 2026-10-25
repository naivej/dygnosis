//! Guarded macro choices shared by source discovery and configured execution.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::span::Span;

/// Request-local macro choices. Source acquisition still owns include paths.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalysisConfiguration {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub defines: Vec<InitialDefinition>,
    #[serde(default)]
    pub overrides: Vec<AnalysisOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equation_selection: Option<EquationSelection>,
}

/// Explicit invocation tag values. This input never performs a file lookup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EquationSelection {
    pub mode: EquationSelectionMode,
    #[serde(default = "default_equation_tag")]
    pub tag: String,
    pub values: Vec<String>,
}

fn default_equation_tag() -> String {
    "name".into()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EquationSelectionMode {
    Include,
    Exclude,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InitialDefinition {
    pub name: String,
    pub expression: String,
}

/// `file: null` denotes unnamed root text, never a filesystem lookup.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GuardedTarget {
    pub file: Option<String>,
    pub site_id: String,
    pub source_revision: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AnalysisOverride {
    Definition {
        target: GuardedTarget,
        expression: String,
    },
    SkipDefinition {
        target: GuardedTarget,
    },
    Branch {
        target: GuardedTarget,
        /// Null selects no arm of this chain.
        branch_id: Option<String>,
    },
}

impl AnalysisOverride {
    pub fn target(&self) -> &GuardedTarget {
        match self {
            Self::Definition { target, .. }
            | Self::SkipDefinition { target }
            | Self::Branch { target, .. } => target,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectiveKind {
    Definition,
    FunctionDefinition,
    Conditional,
}

/// A complete written target. These records do not establish model acceptance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectiveSite {
    pub target: GuardedTarget,
    pub kind: DirectiveKind,
    /// Complete definition or conditional chain, including its closing directive.
    pub span: Span,
    pub directive_span: Span,
    /// Only the opener is copied. Nested bodies remain spans in shared source.
    pub expected_text: String,
    pub name: Option<String>,
    pub branches: Vec<BranchSite>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSite {
    pub branch_id: String,
    pub directive_span: Span,
    pub body_span: Span,
    pub expected_text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceAvailability {
    Active,
    Inactive,
    Mixed,
    #[default]
    Unknown,
}

/// A refusal is separate from a model result; callers must not substitute Source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConfigurationError {
    pub code: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<GuardedTarget>,
}

impl ConfigurationError {
    pub(crate) fn at(mut self, target: &GuardedTarget) -> Self {
        self.target = Some(target.clone());
        self
    }
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            target: None,
        }
    }
}

impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigurationError {}

impl AnalysisConfiguration {
    pub fn preview_edit_cause(&self, action: &str) -> String {
        let mut kinds = Vec::new();
        let branches = self
            .overrides
            .iter()
            .any(|operation| matches!(operation, AnalysisOverride::Branch { .. }));
        let definitions = self
            .overrides
            .iter()
            .any(|operation| !matches!(operation, AnalysisOverride::Branch { .. }));
        if branches {
            kinds.push("branch previews");
        }
        if definitions {
            kinds.push("definition previews");
        }
        if self.equation_selection.is_some() {
            kinds.push("equation selection");
        }
        let recovery = if definitions {
            "Reset Analysis Configuration"
        } else if branches && self.equation_selection.is_some() {
            "Clear branch previews and equation selection in Analysis Configuration"
        } else if branches {
            "Clear branch previews in Analysis Configuration"
        } else {
            "Clear equation selection in Analysis Configuration"
        };
        format!(
            "ANALYSIS_PREVIEW_EDIT_UNSUPPORTED: {action} is unavailable with {}. {recovery}, then request it again. No edit proposals were returned.",
            kinds.join(" and ")
        )
    }

    /// Bounded display of retained choices; does not inspect or evaluate source.
    pub fn display_label(&self) -> String {
        fn short(text: &str) -> String {
            let mut result: String = text.chars().take(70).collect();
            if result.len() < text.len() {
                result.push('…');
            }
            result
        }
        if let Some(name) = self.name.as_deref()
            && !matches!(
                name.trim_end_matches(" (modified)"),
                "Analysis preview" | "Macro inputs"
            )
        {
            return short(name);
        }
        let mut parts: Vec<_> = self
            .defines
            .iter()
            .take(2)
            .map(|input| short(&format!("{}={}", input.name, input.expression)))
            .collect();
        if self.defines.len() > 2 {
            parts.push(format!("+{} macro inputs", self.defines.len() - 2));
        }
        let mut counts = [0usize; 3];
        for operation in &self.overrides {
            counts[match operation {
                AnalysisOverride::Branch { .. } => 0,
                AnalysisOverride::Definition { .. } => 1,
                AnalysisOverride::SkipDefinition { .. } => 2,
            }] += 1;
        }
        for (count, label) in counts.into_iter().zip([
            "branch preview",
            "definition override",
            "skipped definition",
        ]) {
            if count > 0 {
                parts.push(format!(
                    "{count} {label}{}",
                    if count == 1 { "" } else { "s" }
                ));
            }
        }
        if let Some(selection) = &self.equation_selection {
            parts.push(short(&format!(
                "{} {}={}{}",
                if selection.mode == EquationSelectionMode::Include {
                    "Include"
                } else {
                    "Exclude"
                },
                selection.tag,
                selection
                    .values
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
                if selection.values.len() > 2 {
                    format!(" (+{})", selection.values.len() - 2)
                } else {
                    String::new()
                }
            )));
        }
        if parts.is_empty() {
            "Source".into()
        } else {
            parts
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" · ")
                + &if parts.len() > 2 {
                    format!(" · +{} choices", parts.len() - 2)
                } else {
                    String::new()
                }
        }
    }

    pub fn is_preview(&self) -> bool {
        !self.overrides.is_empty() || self.equation_selection.is_some()
    }

    pub fn is_source(&self) -> bool {
        self.defines.is_empty() && self.overrides.is_empty() && self.equation_selection.is_none()
    }

    /// Site choices have no request-order effect; initial definitions do.
    /// The display name does not identify effective analysis inputs.
    /// Call after input validation has applied the root work allowance.
    pub fn configuration_id(&self) -> String {
        let mut overrides: Vec<_> = self.overrides.iter().collect();
        overrides.sort_by(|left, right| {
            let left = left.target();
            let right = right.target();
            (&left.file, &left.site_id).cmp(&(&right.file, &right.site_id))
        });
        let bytes = if let Some(selection) = &self.equation_selection {
            let mut values: Vec<_> = selection.values.iter().collect();
            values.sort();
            values.dedup();
            serde_json::to_vec(&(
                &self.defines,
                overrides,
                selection.mode,
                &selection.tag,
                values,
            ))
        } else {
            serde_json::to_vec(&(&self.defines, overrides))
        }
        .expect("typed macro choices are serializable");
        source_revision(std::str::from_utf8(&bytes).expect("JSON is UTF-8"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnalysisSelection {
    pub configuration_id: String,
    pub name: Option<String>,
    pub preview: bool,
    pub invocation_scope: &'static str,
}

#[derive(Clone, Debug)]
pub enum AnalysisSelectionState {
    Selected(AnalysisSelection),
    Stale {
        selection: AnalysisSelection,
        cause: ConfigurationError,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideApplication {
    Applied,
    NotReached,
    Unknown,
}

#[derive(Clone, Debug, Serialize)]
pub struct OverrideReceipt {
    pub operation: AnalysisOverride,
    /// Written macro name retained by the guarded target; conditionals have none.
    pub name: Option<String>,
    /// Label captured when the guarded branch was accepted, including its opener.
    pub branch_label: Option<String>,
    /// Accepted one-based line, only when the source owner already has an index.
    pub source_line: Option<u32>,
    pub application: OverrideApplication,
    /// Successful applications in the reached prefix, never failed attempts.
    pub known_execution_count: u64,
    pub execution_count: Option<u64>,
    pub cause: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AnalysisMetadata {
    pub selection: AnalysisSelection,
    pub configuration: AnalysisConfiguration,
    pub revision: String,
    pub macro_complete: bool,
    pub model_complete: bool,
    pub overrides: Vec<OverrideReceipt>,
}

#[derive(Clone, Debug)]
pub struct AvailabilityCause {
    pub code: &'static str,
    pub message: String,
    pub file: Option<String>,
    pub span: Option<Span>,
    pub site: Option<GuardedTarget>,
    pub branch_id: Option<String>,
}

#[derive(Clone, Debug)]
pub struct SourceLoopOccurrence {
    pub frame_id: u64,
    pub directive_span: Span,
    pub variable: Option<String>,
    pub value: Option<String>,
}

#[derive(Clone, Debug)]
pub enum SourceFileSelection {
    Root,
    File(String),
}

#[derive(Clone, Debug, Default)]
pub struct SourceStructureQuery {
    pub files: Vec<SourceFileSelection>,
}

#[derive(Clone, Debug)]
pub struct SourceItemOccurrence {
    pub occurrence_id: String,
    pub include_call: u64,
    pub parent_include_call: Option<u64>,
    pub loops: Vec<SourceLoopOccurrence>,
    pub availability: SourceAvailability,
    pub cause: Option<Arc<AvailabilityCause>>,
}

#[derive(Clone, Debug)]
pub struct SourceItemAvailability {
    pub source_id: String,
    pub availability: SourceAvailability,
    /// Outline control rows describe their arm, while source inspection keeps
    /// describing execution of the directive itself.
    pub(crate) control_availability: SourceAvailability,
    pub(crate) control_cause: Option<Arc<AvailabilityCause>>,
    pub cause: Option<Arc<AvailabilityCause>>,
    pub occurrences: Vec<SourceItemOccurrence>,
}

pub struct SourceFileStructure {
    pub file: Option<String>,
    pub source_revision: String,
    pub text: Arc<String>,
    pub(crate) inventory: Arc<crate::engine::analysis::source_inventory::SourceInventory>,
    pub sites: Arc<[DirectiveSite]>,
    pub availability: Arc<[SourceItemAvailability]>,
}

impl SourceFileStructure {
    pub fn source_item_count(&self) -> usize {
        self.inventory.items.len()
    }
}

#[derive(Clone, Debug)]
pub struct IndexedSourceCoverage {
    pub file: Option<String>,
    pub source_revision: Option<String>,
    pub acquired: bool,
    pub executed: bool,
    pub explicitly_selected: bool,
    pub complete: bool,
    pub cause: Option<String>,
}

#[derive(Clone, Debug)]
pub struct UnresolvedSourceTarget {
    pub file: Option<String>,
    pub span: Span,
    pub expression: String,
    pub literal_target: Option<String>,
    pub cause: String,
    pub lookup_context: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SourceCoverage {
    pub indexed_files: Vec<IndexedSourceCoverage>,
    pub unresolved_targets: Vec<UnresolvedSourceTarget>,
    pub complete: bool,
}

pub struct SourceStructure {
    pub selection_state: AnalysisSelectionState,
    pub revision: String,
    pub analysis_revision: Option<String>,
    pub files: Vec<SourceFileStructure>,
    pub coverage: SourceCoverage,
    pub input_guards: Vec<crate::engine::workspace::InputGuard>,
}

#[derive(Clone, Debug)]
pub(crate) struct MacroAvailabilityRegion {
    pub file: String,
    pub span: Span,
    pub active: bool,
    pub control_active: Option<bool>,
    pub include_call: u64,
    pub parent_include_call: Option<u64>,
    pub loops: Vec<SourceLoopOccurrence>,
}

#[derive(Debug, Default)]
struct FileAvailabilityIndex {
    entries: Vec<usize>,
    max_end: Vec<u32>,
    summary: Vec<(Span, SourceAvailability)>,
}

#[derive(Debug, Default)]
pub(crate) struct MacroAvailabilityIndex {
    regions: Arc<[MacroAvailabilityRegion]>,
    files: HashMap<String, FileAvailabilityIndex>,
}

impl MacroAvailabilityIndex {
    pub(crate) fn build(
        regions: Arc<[MacroAvailabilityRegion]>,
        budget: &mut MacroBudget,
    ) -> Result<Arc<Self>, crate::engine::language::macro_expand::MacroEvalError> {
        let sort = regions
            .len()
            .saturating_mul(regions.len().checked_ilog2().unwrap_or(0) as usize + 1);
        budget.spend_work(
            sort.saturating_mul(3)
                .saturating_add(
                    regions
                        .iter()
                        .map(|region| region.file.len().saturating_add(8))
                        .sum::<usize>(),
                )
                .max(1),
        )?;
        let mut files: HashMap<String, FileAvailabilityIndex> = HashMap::new();
        for (index, region) in regions.iter().enumerate() {
            files
                .entry(region.file.clone())
                .or_default()
                .entries
                .push(index);
        }
        for file in files.values_mut() {
            file.entries
                .sort_by_key(|index| (regions[*index].span.start, regions[*index].span.end));
            let mut maximum = 0;
            let mut events = Vec::with_capacity(file.entries.len().saturating_mul(2));
            for &index in &file.entries {
                let region = &regions[index];
                maximum = maximum.max(region.span.end);
                file.max_end.push(maximum);
                if region.span.end > region.span.start {
                    events.push((
                        region.span.start,
                        if region.active { 1i64 } else { 0 },
                        if region.active { 0 } else { 1i64 },
                    ));
                    events.push((
                        region.span.end,
                        if region.active { -1i64 } else { 0 },
                        if region.active { 0 } else { -1i64 },
                    ));
                }
            }
            events.sort_by_key(|event| event.0);
            let (mut active, mut inactive, mut last) = (0i64, 0i64, 0u32);
            for (at, add_active, add_inactive) in events {
                if last < at {
                    let state = match (active > 0, inactive > 0) {
                        (true, true) => SourceAvailability::Mixed,
                        (true, false) => SourceAvailability::Active,
                        (false, true) => SourceAvailability::Inactive,
                        _ => SourceAvailability::Unknown,
                    };
                    if let Some((span, previous)) = file.summary.last_mut()
                        && *previous == state
                        && span.end == last
                    {
                        span.end = at;
                    } else {
                        file.summary.push((
                            Span {
                                start: last,
                                end: at,
                            },
                            state,
                        ));
                    }
                }
                active += add_active;
                inactive += add_inactive;
                last = at;
            }
        }
        Ok(Arc::new(Self { regions, files }))
    }

    pub(crate) fn summary(&self, file: &str, span: Span, complete: bool) -> SourceAvailability {
        if !complete {
            return SourceAvailability::Unknown;
        }
        let Some(file) = self.files.get(file) else {
            return SourceAvailability::Unknown;
        };
        let start = file
            .summary
            .partition_point(|(region, _)| region.end <= span.start);
        let mut cursor = span.start;
        let (mut active, mut inactive) = (false, false);
        for &(region, state) in file
            .summary
            .iter()
            .skip(start)
            .take_while(|(region, _)| region.start < span.end)
        {
            if region.start > cursor || state == SourceAvailability::Unknown {
                return SourceAvailability::Unknown;
            }
            active |= matches!(
                state,
                SourceAvailability::Active | SourceAvailability::Mixed
            );
            inactive |= matches!(
                state,
                SourceAvailability::Inactive | SourceAvailability::Mixed
            );
            cursor = cursor.max(region.end);
        }
        if cursor < span.end {
            return SourceAvailability::Unknown;
        }
        match (active, inactive) {
            (true, true) => SourceAvailability::Mixed,
            (true, false) => SourceAvailability::Active,
            (false, true) => SourceAvailability::Inactive,
            _ => SourceAvailability::Unknown,
        }
    }

    pub(crate) fn matching<'a>(
        &'a self,
        file: &str,
        span: Span,
    ) -> impl Iterator<Item = &'a MacroAvailabilityRegion> + 'a {
        let file = self.files.get(file);
        let end = file.map_or(0, |file| {
            file.entries
                .partition_point(|index| self.regions[*index].span.start <= span.start)
        });
        (0..end)
            .rev()
            .take_while(move |index| file.is_some_and(|file| file.max_end[*index] >= span.end))
            .filter_map(move |index| {
                let region = &self.regions[file.expect("bounded file index").entries[index]];
                (region.span.start <= span.start && span.end <= region.span.end).then_some(region)
            })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ResolvedOverride {
    pub file: String,
    pub operation: AnalysisOverride,
    pub directive_span: Span,
    pub name: Option<String>,
    pub branch_index: Option<usize>,
    /// Accepted picker text, including the controlling opener for a later arm.
    pub branch_label: Option<String>,
    pub source_line: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct ResolvedAnalysis {
    pub choices: AnalysisConfiguration,
    pub selection: AnalysisSelection,
    pub initial: HashMap<String, crate::engine::language::macro_expand::MacroVal>,
    pub overrides: Vec<ResolvedOverride>,
    pub by_file: HashMap<String, Arc<HashMap<Span, usize>>>,
}

/// One original file or reconstructed buffer's lookup into the same choices.
#[derive(Clone, Debug)]
pub(crate) struct MacroExecutionInputs {
    pub analysis: Arc<ResolvedAnalysis>,
    pub sites: Arc<HashMap<Span, usize>>,
}

impl MacroExecutionInputs {
    pub(crate) fn initial_environment(
        &self,
        budget: &mut MacroBudget,
    ) -> Result<
        HashMap<String, crate::engine::language::macro_expand::MacroVal>,
        crate::engine::language::macro_expand::MacroEvalError,
    > {
        let mut initial = HashMap::new();
        for (name, value) in &self.analysis.initial {
            budget.spend_work(name.len().saturating_add(1))?;
            initial.insert(
                name.clone(),
                crate::engine::language::macro_expr::clone_macro_val(value, budget)?,
            );
        }
        Ok(initial)
    }
}

impl ResolvedAnalysis {
    pub(crate) fn for_file(self: &Arc<Self>, file: &str) -> Arc<MacroExecutionInputs> {
        Arc::new(MacroExecutionInputs {
            analysis: Arc::clone(self),
            sites: self.by_file.get(file).cloned().unwrap_or_default(),
        })
    }
}

impl AnalysisConfiguration {
    pub(crate) fn validate_initial(
        &self,
        budget: &mut MacroBudget,
    ) -> Result<HashMap<String, crate::engine::language::macro_expand::MacroVal>, ConfigurationError>
    {
        charge(
            budget,
            self.name
                .as_ref()
                .map_or(1, |name| name.len().saturating_add(1)),
        )?;
        let mut names = HashSet::new();
        if let Some(selection) = &self.equation_selection {
            charge(budget, selection.tag.len().saturating_add(1))?;
            let mut bytes = selection.tag.bytes();
            if !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
                || selection.values.is_empty()
            {
                return Err(ConfigurationError::new(
                    "ANALYSIS_INVALID_EQUATION_SELECTION",
                    "Equation selection requires a tag name and at least one explicit tag value. Selector files are not supported; supply tag values directly.",
                ));
            }
            for value in &selection.values {
                charge(budget, value.len().saturating_add(1))?;
                if value.is_empty() {
                    return Err(ConfigurationError::new(
                        "ANALYSIS_INVALID_EQUATION_SELECTION",
                        "Equation selection contains an empty tag value. Supply nonempty explicit tag values; no selection was changed.",
                    ));
                }
            }
        }
        let mut targets = HashSet::new();
        let mut initial = HashMap::new();
        for define in &self.defines {
            charge(
                budget,
                define
                    .name
                    .len()
                    .saturating_add(define.expression.len())
                    .saturating_add(4),
            )?;
            let mut bytes = define.name.bytes();
            if !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
                || !bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            {
                return Err(ConfigurationError::new(
                    "ANALYSIS_INVALID_DEFINE",
                    format!("Initial macro name '{}' is not a valid name.", define.name),
                ));
            }
            if !names.insert(define.name.as_str()) {
                return Err(ConfigurationError::new(
                    "ANALYSIS_DUPLICATE_DEFINE",
                    format!(
                        "Initial macro name '{}' occurs more than once.",
                        define.name
                    ),
                ));
            }
            crate::engine::language::macro_expr::check_macro_definition_budget(
                &format!("{} = {}", define.name, define.expression),
                budget,
            )
            .map_err(expression_error)?;
            let value = crate::engine::language::macro_expr::eval_macro_expr_budget(
                &define.expression,
                &mut initial,
                budget,
            )
            .map_err(expression_error)?;
            initial.insert(define.name.clone(), value);
        }
        for operation in &self.overrides {
            let target = operation.target();
            charge(
                budget,
                target
                    .file
                    .as_ref()
                    .map_or(0, String::len)
                    .saturating_add(target.site_id.len())
                    .saturating_add(target.source_revision.len())
                    .saturating_add(1),
            )?;
            if let AnalysisOverride::Branch {
                branch_id: Some(branch),
                ..
            } = operation
            {
                charge(budget, branch.len().saturating_add(1))?;
            }
            if !targets.insert((&target.file, &target.site_id)) {
                return Err(ConfigurationError::new(
                    "ANALYSIS_TARGET_CONFLICT",
                    "More than one override selects the same written directive.",
                )
                .at(target));
            }
            if let AnalysisOverride::Definition { expression, .. } = operation {
                crate::engine::language::macro_expr::check_macro_syntax_budget(expression, budget)
                    .map_err(|error| expression_error(error).at(target))?;
            }
        }
        Ok(initial)
    }
}

pub(crate) fn resolve_override(
    operation: &AnalysisOverride,
    file: String,
    source: &crate::engine::language::macro_source::MacroSource,
    budget: &mut MacroBudget,
) -> Result<ResolvedOverride, ConfigurationError> {
    let target = operation.target();
    if source.source_revision != target.source_revision {
        return Err(ConfigurationError::new(
            "ANALYSIS_TARGET_STALE",
            "The override's source file changed. Select the written directive again.",
        )
        .at(target));
    }
    charge(budget, source.sites.len().saturating_add(1))?;
    let site = source
        .sites
        .iter()
        .find(|site| site.target.site_id == target.site_id)
        .ok_or_else(|| {
            ConfigurationError::new(
                "ANALYSIS_TARGET_MISSING",
                "The written directive is missing or has incomplete macro syntax.",
            )
            .at(target)
        })?;
    let branch_index = match operation {
        AnalysisOverride::Definition { .. } | AnalysisOverride::SkipDefinition { .. } => {
            if site.kind == DirectiveKind::FunctionDefinition {
                return Err(ConfigurationError::new("ANALYSIS_TARGET_UNSUPPORTED", "Macro function definitions cannot be overridden. Select a variable definition.").at(target));
            }
            if site.kind != DirectiveKind::Definition {
                return Err(ConfigurationError::new(
                    "ANALYSIS_TARGET_WRONG_KIND",
                    "This override requires a written variable definition.",
                )
                .at(target));
            }
            None
        }
        AnalysisOverride::Branch { branch_id, .. } => {
            if site.kind != DirectiveKind::Conditional {
                return Err(ConfigurationError::new(
                    "ANALYSIS_TARGET_WRONG_KIND",
                    "This override requires a written conditional chain.",
                )
                .at(target));
            }
            match branch_id {
                Some(id) => Some(
                    site.branches
                        .iter()
                        .position(|branch| branch.branch_id == *id)
                        .ok_or_else(|| {
                            ConfigurationError::new(
                                "ANALYSIS_BRANCH_MISSING",
                                "The selected branch does not belong to this conditional chain.",
                            )
                            .at(target)
                        })?,
                ),
                None => None,
            }
        }
    };
    charge(
        budget,
        file.len()
            .saturating_add(site.expected_text.len())
            .saturating_add(
                branch_index.map_or(0, |index| site.branches[index].expected_text.len()),
            )
            .saturating_add(if matches!(operation, AnalysisOverride::Branch { .. }) {
                16
            } else {
                0
            })
            .saturating_add(target.site_id.len())
            .saturating_add(target.source_revision.len())
            .saturating_add(match operation {
                AnalysisOverride::Definition { expression, .. } => expression.len(),
                _ => 0,
            })
            .saturating_add(1),
    )?;
    let branch_label = matches!(operation, AnalysisOverride::Branch { .. }).then(|| {
        let selected = branch_index
            .map(|index| site.branches[index].expected_text.as_str())
            .unwrap_or("No branch");
        if selected == site.expected_text {
            site.expected_text.clone()
        } else {
            format!("{} → {selected}", site.expected_text)
        }
    });
    Ok(ResolvedOverride {
        file,
        operation: operation.clone(),
        directive_span: site.directive_span,
        name: site.name.clone(),
        branch_index,
        branch_label,
        source_line: None,
    })
}

pub(crate) fn expression_error(
    error: crate::engine::language::macro_expand::MacroEvalError,
) -> ConfigurationError {
    use crate::engine::language::macro_expand::MacroEvalError;
    match error {
        MacroEvalError::Limit(name) => ConfigurationError::new(
            "ANALYSIS_CONFIGURATION_LIMIT",
            format!("Analysis configuration validation stopped at the {name} limit."),
        ),
        error => ConfigurationError::new(
            "ANALYSIS_INVALID_EXPRESSION",
            error
                .diagnostic()
                .map(|(_, message)| message)
                .unwrap_or_else(|| "The macro expression syntax is not supported.".to_owned()),
        ),
    }
}

pub(crate) fn charge(budget: &mut MacroBudget, amount: usize) -> Result<(), ConfigurationError> {
    budget.spend_work(amount.max(1)).map_err(expression_error)
}

/// Guard the exact original text, including comments and newline spelling.
pub fn source_revision(source: &str) -> String {
    format!("{:x}", Sha256::digest(source.as_bytes()))
}

pub(crate) fn written_id(revision: &str, kind: &str, span: Span) -> String {
    source_revision(&format!("{revision}:{kind}:{}:{}", span.start, span.end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::language::macro_expand::{discover_directive_sites, scan_macro_tokens};

    fn sites(source: &str) -> Vec<DirectiveSite> {
        discover_directive_sites(
            None,
            source,
            &scan_macro_tokens(source),
            &mut MacroBudget::new(),
        )
        .unwrap()
    }

    #[test]
    fn identity_preserves_input_order_and_ignores_labels_and_site_request_order() {
        // Guards a renamed preset invalidating effective identity, and ordered
        // initial expressions or different overrides sharing one identity.
        let sites = sites("@#define A = 1\n@#define B = 2\n");
        let mut config = AnalysisConfiguration {
            name: Some("First label".into()),
            defines: vec![
                InitialDefinition {
                    name: "A".into(),
                    expression: "1".into(),
                },
                InitialDefinition {
                    name: "B".into(),
                    expression: "A+1".into(),
                },
            ],
            overrides: sites
                .iter()
                .map(|site| AnalysisOverride::SkipDefinition {
                    target: site.target.clone(),
                })
                .collect(),
            equation_selection: None,
        };
        let first = config.configuration_id();
        config.name = Some("Renamed label".into());
        config.overrides.reverse();
        assert_eq!(first, config.configuration_id());
        config.defines.reverse();
        assert_ne!(first, config.configuration_id());
        config.defines.reverse();
        config.overrides.pop();
        assert_ne!(first, config.configuration_id());
        assert!(config.is_preview());
        assert!(!config.is_source());
        let source = AnalysisConfiguration::default();
        assert!(source.is_source());
        assert!(!source.is_preview());
    }
}
