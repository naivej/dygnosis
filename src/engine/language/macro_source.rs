//! Immutable written macro scan shared by execution and source intelligence.

use std::sync::Arc;

use crate::engine::analysis_configuration::{DirectiveSite, source_revision};
use crate::engine::language::lexer::Token;
use crate::engine::language::macro_expand::{
    MacroEvalError, scan_macro_tokens, source_syntax_sites,
};
use crate::engine::language::macro_expr::MacroBudget;
use crate::engine::language::span::Span;

#[derive(Clone, Debug)]
pub(crate) struct MacroSyntaxRefusal {
    pub span: Span,
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug)]
pub(crate) struct MacroSource {
    pub original: Arc<String>,
    pub normalized: Arc<String>,
    pub source_revision: String,
    pub tokens: Arc<[Token]>,
    pub sites: Arc<[DirectiveSite]>,
    pub syntax_refusal: Option<MacroSyntaxRefusal>,
    pub work_used: usize,
}

impl MacroSource {
    /// Only Workspace's completed original-file walk and trusted splicer use
    /// this constructor. Joined text has no file-local revision or target sites:
    /// those facts remain on the acquired original documents. Its macro syntax
    /// is proved by the completed walk, including every original file, and the
    /// splicer preserves that structure while copying or unrolling bodies.
    pub(crate) fn from_completed_splice(
        original: Arc<String>,
        budget: &mut MacroBudget,
    ) -> Result<Arc<Self>, MacroEvalError> {
        let before = budget.remaining_work();
        budget.spend_work(original.len().saturating_mul(2).max(1))?;
        let normalized = Arc::new(crate::engine::language::parser::normalize_newlines(
            &original,
        ));
        let tokens = scan_macro_tokens(&normalized);
        budget.spend_work(1)?;
        Ok(Arc::new(Self {
            original,
            normalized,
            source_revision: String::new(),
            tokens: tokens.into(),
            sites: Arc::from([]),
            syntax_refusal: None,
            work_used: before - budget.remaining_work(),
        }))
    }

    pub(crate) fn build(
        original: Arc<String>,
        budget: &mut MacroBudget,
    ) -> Result<Arc<Self>, MacroEvalError> {
        let before = budget.remaining_work();
        budget.spend_work(original.len().saturating_mul(3).max(1))?;
        let revision = source_revision(&original);
        let normalized = Arc::new(crate::engine::language::parser::normalize_newlines(
            &original,
        ));
        let tokens = scan_macro_tokens(&normalized);
        let (mut sites, syntax_refusal) =
            source_syntax_sites(&normalized, &revision, &tokens, budget)?;
        for site in &mut sites {
            // Newline normalization preserves offsets. Guards and displayed
            // directive text still describe the exact original bytes.
            let raw =
                &original[site.directive_span.start as usize..site.directive_span.end as usize];
            if site.expected_text != raw {
                budget.spend_work(raw.len().max(1))?;
                site.expected_text = raw.to_owned();
            }
            for branch in &mut site.branches {
                let raw = &original
                    [branch.directive_span.start as usize..branch.directive_span.end as usize];
                if branch.expected_text != raw {
                    budget.spend_work(raw.len().max(1))?;
                    branch.expected_text = raw.to_owned();
                }
            }
        }
        budget.spend_work(1)?;
        Ok(Arc::new(Self {
            original,
            normalized,
            source_revision: revision,
            tokens: tokens.into(),
            sites: sites.into(),
            syntax_refusal,
            work_used: before - budget.remaining_work(),
        }))
    }
}
