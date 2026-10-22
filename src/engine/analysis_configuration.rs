//! Guarded macro choices shared by source discovery and configured execution.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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
    pub fn is_preview(&self) -> bool {
        !self.overrides.is_empty()
    }

    pub fn is_source(&self) -> bool {
        self.defines.is_empty() && self.overrides.is_empty()
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
        let bytes = serde_json::to_vec(&(&self.defines, overrides))
            .expect("typed macro choices are serializable");
        source_revision(std::str::from_utf8(&bytes).expect("JSON is UTF-8"))
    }
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
