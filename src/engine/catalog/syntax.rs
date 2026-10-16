//! Shared, source-free reference for the pinned Dynare grammar.
//!
//! Stable kind/context identifiers are strings so that transports can add filters
//! without exposing a second enum inventory. The generated records retain
//! spelling roles separately: `log` in a model and `var(log)` are not one form.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

/// One written option, or an open per-symbol partition-key slot.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SyntaxOption {
    pub name: String,
    pub open_slot: bool,
    pub signatures: Vec<String>,
    #[serde(default)]
    pub value_rules: Vec<String>,
    #[serde(default)]
    pub description: String,
    pub grammar_sources: Vec<String>,
}

/// One syntax spelling in one accepted role and context.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SyntaxRecord {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub contexts: Vec<String>,
    pub parent: Option<String>,
    pub signatures: Vec<String>,
    #[serde(default)]
    pub value_rules: Vec<String>,
    pub summary: String,
    pub aliases: Vec<String>,
    pub options: Vec<SyntaxOption>,
    pub grammar_sources: Vec<String>,
}

/// Exact name and independent narrowing filters. Search is a literal substring.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SyntaxQuery {
    pub name: Option<String>,
    pub search: Option<String>,
    pub kind: Option<String>,
    pub context: Option<String>,
    pub parent: Option<String>,
}

/// Deterministic results before transport paging.
#[derive(Clone, Debug, Serialize)]
pub struct SyntaxQueryResult {
    pub records: Vec<SyntaxRecord>,
    /// Whether the exact name exists before applying context/parent/kind filters.
    pub known_name: bool,
    pub available_contexts: Vec<String>,
}

/// Complete finite reference inventory. It contains no source-model analysis.
pub fn records() -> &'static [SyntaxRecord] {
    static RECORDS: OnceLock<Vec<SyntaxRecord>> = OnceLock::new();
    RECORDS.get_or_init(|| {
        let mut rows: Vec<SyntaxRecord> = serde_json::from_str(include_str!("syntax_data.json"))
            .expect("bundled pinned syntax records must deserialize");
        for row in &mut rows {
            if let Some(help) = super::family_help(&row.name) {
                row.summary = help.to_owned();
            }
            for option in &mut row.options {
                if let Some((_, description)) = super::command_options(&row.name)
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&option.name))
                    && !description.is_empty()
                    && !description_is_fragment(description)
                {
                    option.description = (*description).to_owned();
                }
            }
            if matches!(row.kind.as_str(), "option" | "open_slot")
                && let Some(parent) = &row.parent
                && let Some((_, description)) = super::command_options(parent)
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(&row.name))
                && !description.is_empty()
                && !description_is_fragment(description)
            {
                row.summary = (*description).to_owned();
            }
        }
        rows
    })
}

fn description_is_fragment(description: &str) -> bool {
    let text = description.trim();
    text.ends_with('(')
        || text.ends_with("see")
        || text.ends_with("...")
        || text.ends_with("i.e.")
        || text.ends_with("e.g.")
        || text.contains("<first_obs =")
        || text.contains(":noindex:")
        || text.matches('(').count() != text.matches(')').count()
}

/// Query exact names/aliases case-insensitively; keep spelling roles separate.
pub fn query(query: &SyntaxQuery) -> SyntaxQueryResult {
    let name_matches = |row: &&SyntaxRecord| {
        query.name.as_ref().is_none_or(|name| {
            row.name.eq_ignore_ascii_case(name)
                || row
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(name))
                || (row.kind == "macro_directive"
                    && row.name.trim_start_matches("@#").eq_ignore_ascii_case(name))
        })
    };
    let candidates: Vec<_> = records().iter().filter(name_matches).collect();
    let mut available_contexts: Vec<_> = candidates
        .iter()
        .flat_map(|row| row.contexts.iter().cloned())
        .collect();
    available_contexts.sort();
    available_contexts.dedup();
    let known_name = query.name.is_none() || !candidates.is_empty();
    let search = query.search.as_ref().map(|s| s.to_ascii_lowercase());
    let records = candidates
        .into_iter()
        .filter(|row| {
            query
                .kind
                .as_ref()
                .is_none_or(|kind| row.kind.eq_ignore_ascii_case(kind))
                && query.context.as_ref().is_none_or(|context| {
                    row.contexts.iter().any(|c| c.eq_ignore_ascii_case(context))
                })
                && query.parent.as_ref().is_none_or(|parent| {
                    row.parent
                        .as_ref()
                        .is_some_and(|p| p.eq_ignore_ascii_case(parent))
                })
                && search.as_ref().is_none_or(|search| {
                    row.name.to_ascii_lowercase().contains(search)
                        || row.summary.to_ascii_lowercase().contains(search)
                        || row
                            .aliases
                            .iter()
                            .any(|a| a.to_ascii_lowercase().contains(search))
                })
        })
        .cloned()
        .collect();
    SyntaxQueryResult {
        records,
        known_name,
        available_contexts,
    }
}

/// Block spelling and aliases from the same records used by syntax lookup.
pub fn is_block(name: &str) -> bool {
    records().iter().any(|row| {
        row.kind == "block"
            && (row.name.eq_ignore_ascii_case(name)
                || row.aliases.iter().any(|a| a.eq_ignore_ascii_case(name)))
    })
}

/// Finite names in a context, optionally restricted to a role and owning head.
pub fn in_context<'a>(
    context: &'a str,
    kind: Option<&'a str>,
    parent: Option<&'a str>,
) -> impl Iterator<Item = &'static SyntaxRecord> + 'a {
    records().iter().filter(move |row| {
        row.contexts.iter().any(|c| c.eq_ignore_ascii_case(context))
            && kind.is_none_or(|kind| row.kind.eq_ignore_ascii_case(kind))
            && parent.is_none_or(|parent| {
                row.parent
                    .as_ref()
                    .is_some_and(|p| p.eq_ignore_ascii_case(parent))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_records_have_unique_selectors_and_usable_shapes() {
        // Guards ambiguous record selection and incomplete reference payloads after regeneration.
        let mut ids = std::collections::HashSet::new();
        for row in records() {
            assert!(ids.insert(&row.id), "duplicate syntax identity {}", row.id);
            assert!(
                !row.name.is_empty() && !row.contexts.is_empty(),
                "{}",
                row.id
            );
            assert!(
                !row.signatures.is_empty(),
                "{} has no accepted shape",
                row.id
            );
            assert!(!row.grammar_sources.is_empty(), "{} has no source", row.id);
            assert!(
                row.summary
                    != format!(
                        "Dynare {} {}.",
                        row.name.replace('_', " "),
                        row.kind.replace('_', " ")
                    )
                    || matches!(row.kind.as_str(), "operator" | "delimiter"),
                "{} has no explanation",
                row.id
            );
            for option in &row.options {
                assert!(
                    !option.signatures.is_empty() && !option.value_rules.is_empty(),
                    "{}/{}",
                    row.name,
                    option.name
                );
                assert!(
                    !option.grammar_sources.is_empty(),
                    "{}/{}",
                    row.name,
                    option.name
                );
            }
        }
    }

    #[test]
    fn spelling_roles_and_context_refusals_are_distinct() {
        // Guards confusing a model-only function with the equally spelled option.
        let forms = query(&SyntaxQuery {
            name: Some("diff".into()),
            ..Default::default()
        });
        assert!(
            forms
                .records
                .iter()
                .any(|r| r.contexts == ["model_expression"])
        );
        assert!(
            forms
                .records
                .iter()
                .any(|r| r.kind == "option"
                    && r.parent.as_deref() == Some("plot_shock_decomposition"))
        );
        let wrong = query(&SyntaxQuery {
            name: Some("diff".into()),
            context: Some("macro_expression".into()),
            ..Default::default()
        });
        assert!(wrong.known_name);
        assert!(wrong.records.is_empty());
        assert!(
            wrong
                .available_contexts
                .contains(&"model_expression".to_owned())
        );
        let unknown = query(&SyntaxQuery {
            name: Some("not_a_dynare_function".into()),
            ..Default::default()
        });
        assert!(!unknown.known_name);
    }

    #[test]
    fn punctuation_queries_keep_the_consuming_grammar_context() {
        // Guards claiming that block updates, equation relations or macro
        // assignments are ordinary/model/macro expression operators.
        for name in ["+=", "*="] {
            let accepted = query(&SyntaxQuery {
                name: Some(name.into()),
                context: Some("block_body".into()),
                parent: Some("endval".into()),
                ..Default::default()
            });
            assert!(
                accepted
                    .records
                    .iter()
                    .any(|r| r.signatures.iter().any(|s| s.contains(name)))
            );
            for context in ["expression", "model_expression", "macro_expression"] {
                let wrong = query(&SyntaxQuery {
                    name: Some(name.into()),
                    context: Some(context.into()),
                    ..Default::default()
                });
                assert!(
                    wrong.known_name && wrong.records.is_empty(),
                    "{name}: {context}"
                );
            }
            assert!(
                query(&SyntaxQuery {
                    name: Some(name.into()),
                    parent: Some("initval".into()),
                    ..Default::default()
                })
                .records
                .is_empty()
            );
        }
        for name in ["=", "⟂", ";", "@{"] {
            for context in ["expression", "model_expression", "macro_expression"] {
                assert!(
                    query(&SyntaxQuery {
                        name: Some(name.into()),
                        context: Some(context.into()),
                        ..Default::default()
                    })
                    .records
                    .is_empty(),
                    "{name}: {context}"
                );
            }
        }
        assert!(
            !query(&SyntaxQuery {
                name: Some("=".into()),
                context: Some("macro_directive".into()),
                ..Default::default()
            })
            .records
            .is_empty()
        );
        assert!(
            !query(&SyntaxQuery {
                name: Some("⟂".into()),
                context: Some("block_body".into()),
                parent: Some("model".into()),
                ..Default::default()
            })
            .records
            .is_empty()
        );
        assert!(
            !query(&SyntaxQuery {
                name: Some("@{".into()),
                context: Some("macro_interpolation".into()),
                ..Default::default()
            })
            .records
            .is_empty()
        );
        for name in ["==", ",", "(", ")"] {
            for context in ["expression", "model_expression", "macro_expression"] {
                assert!(
                    !query(&SyntaxQuery {
                        name: Some(name.into()),
                        context: Some(context.into()),
                        ..Default::default()
                    })
                    .records
                    .is_empty(),
                    "{name}: {context}"
                );
            }
        }
    }

    #[test]
    fn canonical_option_spelling_survives_case_insensitive_lookup() {
        // Guards lowercasing the official written name in completion/reference data.
        for spelling in ["Nsam", "nsam", "NSAM"] {
            let result = query(&SyntaxQuery {
                name: Some(spelling.into()),
                parent: Some("sensitivity".into()),
                ..Default::default()
            });
            assert!(
                result
                    .records
                    .iter()
                    .any(|r| r.name == "Nsam"
                        && r.signatures.iter().all(|s| s.starts_with("Nsam =")))
            );
        }
        let sensitivity = query(&SyntaxQuery {
            name: Some("sensitivity".into()),
            kind: Some("command".into()),
            ..Default::default()
        });
        assert!(
            sensitivity
                .records
                .iter()
                .any(|r| r.options.iter().any(|o| o.name == "Nsam"))
        );
        assert!(
            super::super::command_options("sensitivity")
                .iter()
                .any(|(name, _)| *name == "Nsam")
        );
    }

    #[test]
    fn aliases_arity_and_macro_casts_come_from_the_pin() {
        // Guards alias lookup, lost alternate arities and model/macro syntax mixing.
        assert!(is_block("PRIORS"));
        let normcdf = query(&SyntaxQuery {
            name: Some("normcdf".into()),
            context: Some("model_expression".into()),
            ..Default::default()
        });
        assert!(
            normcdf.records[0]
                .signatures
                .iter()
                .any(|s| s.matches(',').count() == 2)
        );
        assert!(
            normcdf.records[0]
                .signatures
                .iter()
                .any(|s| !s.contains(','))
        );
        let cast = query(&SyntaxQuery {
            name: Some("real".into()),
            context: Some("macro_expression".into()),
            ..Default::default()
        });
        assert!(
            cast.records[0]
                .signatures
                .iter()
                .any(|s| s.starts_with("(real)"))
        );
        let priors = query(&SyntaxQuery {
            name: Some("priors".into()),
            kind: Some("block".into()),
            ..Default::default()
        });
        assert!(
            priors
                .records
                .iter()
                .any(|r| r.signatures.iter().any(|s| s.starts_with("priors")))
        );
    }

    #[test]
    fn option_keys_and_open_slots_are_safe_to_suggest() {
        // Guards suggesting internal storage keys or a literal generic partition key.
        let model = query(&SyntaxQuery {
            name: Some("model".into()),
            ..Default::default()
        });
        assert!(model.records.iter().any(|r| {
            r.options
                .iter()
                .any(|o| o.name == "heterogeneity" && !o.signatures.is_empty())
        }));
        assert!(
            query(&SyntaxQuery {
                parent: Some("shock_decomposition".into()),
                name: Some("nograph".into()),
                ..Default::default()
            })
            .records
            .iter()
            .all(|r| r.signatures == ["nograph"])
        );
        assert!(
            query(&SyntaxQuery {
                name: Some("shock_decomposition_nograph".into()),
                ..Default::default()
            })
            .records
            .is_empty()
        );
        assert!(
            query(&SyntaxQuery {
                name: Some("<partition_name>".into()),
                parent: Some("var".into()),
                ..Default::default()
            })
            .records
            .iter()
            .all(|r| r.kind == "open_slot")
        );
        assert!(
            !super::super::command_options("var")
                .iter()
                .any(|(n, _)| *n == "name")
        );
        assert!(
            !super::super::command_options("sbvar")
                .iter()
                .any(|(n, _)| *n == "data")
        );
    }
}
