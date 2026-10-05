use std::collections::HashMap;

use dygnosis::{dynare_compare_models, dynare_equations, dynare_model_info};
use serde_json::{json, Value};

fn assert_incomplete_equations(result: &Value, status: &Value) {
    assert_eq!(result["status"], "incomplete", "{result}");
    assert_eq!(result["message"], status["message"]);
    assert_eq!(result["equations"], json!([]));
    assert!(result["count_gap"].is_null(), "{result}");
}

#[test]
fn equations_use_model_info_completeness_for_all_required_inputs() {
    for text in [
        "var y; model; y=1;\n@#include \"missing.inc\"\nend;",
        "var y; model; y=1;",
        "var y; model; y=@{unknown}; end;",
    ] {
        let expected = dynare_model_info(text, None, None);
        assert_eq!(expected["status"], "incomplete", "{expected}");
        for filter in [None, Some(0)] {
            assert_incomplete_equations(
                &dynare_equations(text, None, None, None, filter),
                &expected,
            );
        }
        let files = HashMap::from([("main.mod".to_string(), text.to_string())]);
        let mapped = dynare_model_info(text, Some("main.mod"), Some(&files));
        assert_incomplete_equations(
            &dynare_equations(text, Some("main.mod"), Some(&files), Some("y"), None),
            &mapped,
        );
    }
}

fn assert_incomplete_compare(result: Value) {
    assert_eq!(result["status"], "incomplete", "{result}");
    assert!(result["message"]
        .as_str()
        .is_some_and(|message| !message.is_empty()));
    assert_eq!(
        result.as_object().unwrap().len(),
        2,
        "diff claims escaped: {result}"
    );
}

#[test]
fn comparisons_withhold_all_claims_when_either_side_is_incomplete() {
    let complete = "var y; model; y=1; end;";
    for incomplete in [
        "var y; model; y=2;\n@#include \"missing.inc\"\nend;",
        "var y; model; y=2;",
        "var y; model; y=@{unknown}; end;",
    ] {
        for (before, after) in [
            (incomplete, complete),
            (complete, incomplete),
            (incomplete, incomplete),
        ] {
            assert_incomplete_compare(dynare_compare_models(
                before, after, None, None, None, None, None,
            ));
            let before_files = HashMap::from([("before.mod".to_string(), before.to_string())]);
            let after_files = HashMap::from([("after.mod".to_string(), after.to_string())]);
            assert_incomplete_compare(dynare_compare_models(
                before,
                after,
                Some("before.mod"),
                Some("after.mod"),
                Some(&before_files),
                Some(&after_files),
                None,
            ));
        }
    }
}

#[test]
fn identical_roots_with_one_missing_include_do_not_claim_equation_changes() {
    let text = "var y; model;\n@#include \"body.inc\"\nend;";
    let full = HashMap::from([
        ("main.mod".to_string(), text.to_string()),
        ("body.inc".to_string(), "y=1;".to_string()),
    ]);
    let missing = HashMap::from([("main.mod".to_string(), text.to_string())]);
    for (before, after) in [(&full, &missing), (&missing, &full), (&missing, &missing)] {
        let result = dynare_compare_models(
            text,
            text,
            Some("main.mod"),
            Some("main.mod"),
            Some(before),
            Some(after),
            None,
        );
        assert_eq!(result["message"], "Model expansion is incomplete");
        assert_incomplete_compare(result);
    }
    let complete = dynare_compare_models(
        text,
        text,
        Some("main.mod"),
        Some("main.mod"),
        Some(&full),
        Some(&full),
        None,
    );
    assert!(complete.get("status").is_none(), "{complete}");
    assert!(complete["added_equations"].as_array().unwrap().is_empty());
    assert!(complete["removed_equations"].as_array().unwrap().is_empty());
}
