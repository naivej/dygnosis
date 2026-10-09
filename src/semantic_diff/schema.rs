use std::collections::BTreeMap;

use crate::span::Span;
use serde::Serialize;

/// Additive payload versions do not change either navigation schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparisonVersions {
    pub semantic: u32,
    pub source_changes: u32,
    pub coverage: u32,
}

impl Default for ComparisonVersions {
    fn default() -> Self {
        Self {
            semantic: 1,
            source_changes: 1,
            coverage: 1,
        }
    }
}

/// Limits bound optional detail, not the legacy structural comparison.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparisonBudgets {
    pub token_alignment_cells: usize,
    pub source_alignment_cells: usize,
    pub references_per_side: usize,
    pub source_hunks: usize,
    /// Serialized rows, references and hunks. Fixed availability/file-action
    /// envelopes and the preserved legacy payload are outside this detail cap.
    pub serialized_output_bytes: usize,
}

impl Default for ComparisonBudgets {
    fn default() -> Self {
        Self {
            token_alignment_cells: 250_000,
            source_alignment_cells: 1_000_000,
            references_per_side: 2_000,
            source_hunks: 2_000,
            serialized_output_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Complete,
    Partial,
    NotAvailable,
    LimitExceeded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparisonLimit {
    pub code: String,
    pub reason: String,
    pub omitted: Option<usize>,
    pub owner: String,
}

impl ComparisonLimit {
    pub fn new(code: &str, reason: &str, owner: &str) -> Self {
        Self {
            code: code.into(),
            reason: reason.into(),
            omitted: None,
            owner: owner.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Added,
    Removed,
    Changed,
    Unpaired,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeFacet {
    Expression,
    ParameterValue,
    SymbolKind,
    Label,
    Tags,
    Scope,
    LogTransform,
    PredeterminedConvention,
    Timing,
    Complementarity,
    ShockSetup,
    Target,
    Role,
    Options,
    Order,
    Assignment,
    Prior,
    Operation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CountUnit {
    FinalFact,
    AcceptedOccurrence,
    Operation,
}

/// Families name compared facts, not diagnostic classes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticFamily {
    Symbols,
    Parameters,
    Equations,
    Shocks,
    SteadyState,
    Priors,
    Commands,
    Observables,
    Data,
    Occbin,
    Policy,
    SemiStructural,
    Moments,
    MsSbvar,
    Heterogeneity,
    ExternalFunctions,
    Trends,
    Operations,
    MacroContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Before,
    After,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparisonScope {
    pub domain: String,
    pub dimension: Option<String>,
    pub block: Option<String>,
}

impl ComparisonScope {
    pub fn aggregate() -> Self {
        Self {
            domain: "aggregate".into(),
            dimension: None,
            block: None,
        }
    }
    pub fn global() -> Self {
        Self {
            domain: "global".into(),
            dimension: None,
            block: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StatementContext {
    pub kind: String,
    pub name: String,
    pub execution_order: usize,
    pub scope: ComparisonScope,
    pub pointer: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RowSide {
    pub name: String,
    pub scope: ComparisonScope,
    /// Producer occurrence identity within this captured side and scope. It
    /// does not establish correspondence to the other side or survive Refresh.
    pub occurrence: Option<usize>,
    pub equation_index: Option<usize>,
    pub context: Option<StatementContext>,
    /// Accepted parser provenance for navigation; never a displayed-text offset.
    #[serde(skip)]
    pub provenance: Option<OccurrenceProvenance>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OccurrenceProvenance {
    pub span: Span,
    pub parse_order: Option<usize>,
    pub equation_id: Option<usize>,
}

impl RowSide {
    pub fn named(name: &str, scope: ComparisonScope) -> Self {
        Self {
            name: name.into(),
            scope,
            occurrence: None,
            equation_index: None,
            context: None,
            provenance: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueState {
    Absent,
    Empty,
    Unknown,
    Present,
}

/// Recursive named values keep ordered associations intact, without JSON blobs.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum FieldValue {
    Text(String),
    Number(f64),
    Integer(i64),
    Boolean(bool),
    List(Vec<FieldValue>),
    Record(BTreeMap<String, FieldValue>),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldState {
    pub state: ValueState,
    pub value: Option<FieldValue>,
}

impl FieldState {
    pub fn absent() -> Self {
        Self {
            state: ValueState::Absent,
            value: None,
        }
    }
    pub fn unknown() -> Self {
        Self {
            state: ValueState::Unknown,
            value: None,
        }
    }
    pub fn present(value: FieldValue) -> Self {
        Self {
            state: ValueState::Present,
            value: Some(value),
        }
    }
    pub fn text(value: &str) -> Self {
        Self {
            state: if value.is_empty() {
                ValueState::Empty
            } else {
                ValueState::Present
            },
            value: Some(FieldValue::Text(value.into())),
        }
    }
    pub fn optional_text(value: Option<&str>) -> Self {
        value.map(Self::text).unwrap_or_else(Self::absent)
    }
    pub fn number(value: Option<f64>) -> Self {
        value
            .filter(|value| value.is_finite())
            .map(|value| Self::present(FieldValue::Number(value)))
            .unwrap_or_else(Self::unknown)
    }
    pub fn boolean(value: bool) -> Self {
        Self::present(FieldValue::Boolean(value))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct FieldChange {
    pub name: String,
    pub label: String,
    pub before: FieldState,
    pub after: FieldState,
    pub changed: bool,
    pub numeric_difference: Option<f64>,
}

impl FieldChange {
    pub fn new(name: &str, label: &str, before: FieldState, after: FieldState) -> Self {
        let changed = before != after;
        Self {
            name: name.into(),
            label: label.into(),
            before,
            after,
            changed,
            numeric_difference: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenRole {
    Unchanged,
    Added,
    Removed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HighlightBasis {
    PairedExpression,
    UnpairedTextOnly,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TokenRun {
    pub text: String,
    pub role: TokenRole,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExpressionSide {
    pub text: String,
    pub runs: Vec<TokenRun>,
}

impl ExpressionSide {
    pub fn plain(text: &str) -> Self {
        Self {
            text: text.into(),
            runs: vec![TokenRun {
                text: text.into(),
                role: TokenRole::Unchanged,
            }],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExpressionDetail {
    pub field: String,
    pub before: Option<ExpressionSide>,
    pub after: Option<ExpressionSide>,
    pub highlight_basis: HighlightBasis,
    pub availability: Availability,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TimingSide {
    pub name: String,
    pub class: String,
    pub written_offset: i32,
    pub converted_offset: i32,
    pub occurrence: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TimingChange {
    pub before: Option<TimingSide>,
    pub after: Option<TimingSide>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EquationReference {
    pub pointer: String,
    pub symbol: String,
    pub side: Side,
    pub equation_pointer: String,
    pub equation_index: usize,
    pub label: String,
    pub scope: ComparisonScope,
    pub occurrence: usize,
    /// One side's accepted written use, including convention-only conversion.
    pub timing: TimingSide,
    #[serde(skip)]
    pub provenance: Option<OccurrenceProvenance>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SemanticRow {
    /// Exact legacy pointer, or the independently owned /semantic/rows/N entry.
    pub pointer: String,
    pub family: SemanticFamily,
    pub change: ChangeKind,
    pub name: String,
    pub count_unit: CountUnit,
    pub facets: Vec<ChangeFacet>,
    pub before: Option<RowSide>,
    pub after: Option<RowSide>,
    pub fields: Vec<FieldChange>,
    pub expressions: Vec<ExpressionDetail>,
    pub timing: Vec<TimingChange>,
    pub references: Vec<String>,
    pub limits: Vec<ComparisonLimit>,
}

impl SemanticRow {
    pub fn new(family: SemanticFamily, change: ChangeKind, name: &str) -> Self {
        Self {
            pointer: String::new(),
            family,
            change,
            name: name.into(),
            count_unit: CountUnit::FinalFact,
            facets: Vec::new(),
            before: None,
            after: None,
            fields: Vec::new(),
            expressions: Vec::new(),
            timing: Vec::new(),
            references: Vec::new(),
            limits: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SemanticDiff {
    pub schema_version: u32,
    pub availability: Availability,
    pub budgets: ComparisonBudgets,
    pub rows: Vec<SemanticRow>,
    pub references: Vec<EquationReference>,
    pub limits: Vec<ComparisonLimit>,
    #[serde(skip)]
    pub work: ComparisonWork,
}

impl SemanticDiff {
    pub fn new(budgets: ComparisonBudgets) -> Self {
        Self {
            schema_version: 1,
            availability: Availability::Partial,
            budgets,
            rows: Vec::new(),
            references: Vec::new(),
            limits: Vec::new(),
            work: ComparisonWork::default(),
        }
    }
    pub fn push_row(&mut self, mut row: SemanticRow) -> String {
        if row.pointer.is_empty() {
            row.pointer = format!("/semantic/rows/{}", self.rows.len());
        }
        let pointer = row.pointer.clone();
        self.rows.push(row);
        pointer
    }
}

/// Work charges accumulate across the entire comparison, not each row.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ComparisonWork {
    pub token_alignment_cells: usize,
    pub source_alignment_cells: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FamilyCoverage {
    pub family: SemanticFamily,
    pub availability: Availability,
    pub fields: Vec<String>,
    pub limits: Vec<ComparisonLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComparisonCoverage {
    pub schema_version: u32,
    pub availability: Availability,
    pub source_boundary: String,
    pub families: Vec<FamilyCoverage>,
    pub limits: Vec<ComparisonLimit>,
}

impl Default for ComparisonCoverage {
    fn default() -> Self {
        Self {
            schema_version: 1,
            availability: Availability::Partial,
            source_boundary: "parsed_models_only".into(),
            families: Vec::new(),
            limits: vec![ComparisonLimit::new(
                "capture_boundary_unavailable",
                "Parsed models alone do not establish the captured source boundary.",
                "source_changes",
            )],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceCorrespondence {
    SelectedRoots,
    ProvenFileIdentity,
    Unpaired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceFileSide {
    pub input_id: Option<String>,
    pub file_key: String,
    pub exact_text_available: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceLine {
    pub role: TokenRole,
    pub text: String,
}

/// Line numbers are one-based hunk coordinates, not navigation positions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceHunk {
    pub before_start: u32,
    pub before_lines: u32,
    pub after_start: u32,
    pub after_lines: u32,
    pub lines: Vec<SourceLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceFileChange {
    pub pointer: String,
    pub change: ChangeKind,
    pub correspondence: SourceCorrespondence,
    pub before: Option<SourceFileSide>,
    pub after: Option<SourceFileSide>,
    pub availability: Availability,
    pub hunks: Vec<SourceHunk>,
    pub omitted_hunks: Option<usize>,
    pub limits: Vec<ComparisonLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SourceChanges {
    pub schema_version: u32,
    pub availability: Availability,
    pub files: Vec<SourceFileChange>,
    pub limits: Vec<ComparisonLimit>,
}

impl Default for SourceChanges {
    fn default() -> Self {
        Self {
            schema_version: 1,
            availability: Availability::NotAvailable,
            files: Vec::new(),
            limits: vec![ComparisonLimit::new(
                "captured_sources_required",
                "Source changes require captured roots and executed includes.",
                "source_changes",
            )],
        }
    }
}
