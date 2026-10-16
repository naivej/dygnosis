//! Real catalog/query regressions for the Dynare 7.2 syntax owner.

use std::time::Duration;

use dygnosis::catalog::syntax::{SyntaxQuery, query};
use dygnosis::preprocessor::find_preprocessor;
use dygnosis::{JsonStage, Severity, check_file, command_options, run_preprocessor};

const BASE: &str = "var y;\nvarexo e;\nparameters a;\na=.5;\nmodel;\ny=a*y(-1)+e;\nend;\n";

#[test]
fn corrected_option_suggestions_are_accepted_and_internal_keys_are_refused() {
    // Guards the catalog offering a source spelling that the same parser refuses.
    for (command, option, statement, accepted) in [
        ("stoch_simul", "graph", "stoch_simul(graph);", true),
        (
            "shock_decomposition",
            "nograph",
            "shock_decomposition(nograph);",
            true,
        ),
        (
            "shock_decomposition",
            "shock_decomposition_nograph",
            "shock_decomposition(shock_decomposition_nograph);",
            false,
        ),
        (
            "occbin_setup",
            "likelihood_brute_force_regime_guess",
            "occbin_setup(likelihood_brute_force_regime_guess);",
            false,
        ),
        ("sbvar", "data", "sbvar(data='test.csv');", false),
    ] {
        assert_eq!(
            command_options(command)
                .iter()
                .any(|(name, _)| *name == option),
            accepted,
            "{command}/{option}"
        );
        let source = format!("{BASE}{statement}\n");
        let diagnostics = check_file(&source, "catalog_syntax.mod");
        assert_eq!(
            diagnostics
                .iter()
                .any(|d| d.code == "E001" && d.severity == Severity::Error),
            !accepted,
            "{statement}: {diagnostics:?}"
        );
        if let Some(pp) = find_preprocessor(None) {
            let official = run_preprocessor(
                &source,
                &pp,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert_eq!(official.success, accepted, "{statement}: {official:?}");
        }
    }
}

#[test]
fn model_options_values_and_open_partition_keys_have_distinct_reference_forms() {
    // Guards unknown valid heads, lost direct values and an invented literal `name` suggestion.
    for head in [
        "model",
        "model_options",
        "compilation_setup",
        "priors",
        "histval",
        "var",
    ] {
        let result = query(&SyntaxQuery {
            name: Some(head.into()),
            ..Default::default()
        });
        assert!(result.known_name && !result.records.is_empty(), "{head}");
    }
    let heterogeneity = query(&SyntaxQuery {
        name: Some("heterogeneity".into()),
        parent: Some("model".into()),
        ..Default::default()
    });
    assert!(
        heterogeneity
            .records
            .iter()
            .any(|r| r.signatures == ["heterogeneity = NAME"])
    );
    let partition = query(&SyntaxQuery {
        name: Some("<partition_name>".into()),
        parent: Some("var".into()),
        ..Default::default()
    });
    assert!(
        partition
            .records
            .iter()
            .any(|r| r.kind == "open_slot" && r.value_rules == ["QUOTED_STRING"])
    );
    let block = query(&SyntaxQuery {
        name: Some("priors".into()),
        kind: Some("block".into()),
        ..Default::default()
    });
    assert!(
        block
            .records
            .iter()
            .any(|r| r.options.iter().any(|o| o.name == "overwrite"))
    );
    assert!(
        block
            .records
            .iter()
            .all(|r| !r.options.iter().any(|o| o.name == "prior_mean"))
    );
}
