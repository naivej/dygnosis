use std::{path::PathBuf, time::Duration};

use dygnosis::{
    analyze, find_preprocessor, parse, run_preprocessor, ExprKind, JsonStage, Severity,
};
use serde::Deserialize;

// Exact sources and official sentences from the pinned 644-case reach audit.
// Keep this fixture portable with the public product repository.
#[derive(Deserialize)]
struct Case {
    name: String,
    family: String,
    source: String,
    accepted: bool,
    baseline_reach: String,
    sentences: Vec<String>,
    range: Option<[u32; 2]>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!("fixtures/model_expression_reach.json")).unwrap()
}

fn pinned_binary() -> Option<PathBuf> {
    let pinned = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    if pinned.is_file() {
        return Some(pinned);
    }
    find_preprocessor(None).filter(|path| {
        path.components()
            .any(|part| part.as_os_str().to_string_lossy() == "7.2")
    })
}

#[test]
fn exact_model_expression_reach_regressions_and_accepted_controls() {
    let cases = cases();
    assert_eq!(cases.len(), 644);
    assert_eq!(
        cases
            .iter()
            .filter(|case| case.baseline_reach == "gap")
            .count(),
        276
    );
    assert_eq!(cases.iter().filter(|case| case.accepted).count(), 108);
    let mut failures = Vec::new();
    for case in cases {
        let rows = analyze(&parse(&case.source));
        if case.accepted {
            if rows.iter().any(|row| row.severity == Severity::Error) {
                failures.push(format!("{}: accepted control: {rows:?}", case.name));
            }
            if !case.family.starts_with('W') {
                continue;
            }
        }
        let severity = if case.family.starts_with('W') {
            Severity::Warning
        } else {
            Severity::Error
        };
        // Existing fire rows retain their documented editor ranges. All owned
        // gaps and all existing generic role/timing rows lock written ranges.
        let strict = case.baseline_reach == "gap"
            || matches!(
                case.family.as_str(),
                "E280" | "E281" | "E294" | "E024" | "E182"
            );
        if case.baseline_reach == "gap" {
            assert!(
                case.range.is_some(),
                "{}: owned gap has no written range",
                case.name
            );
        }
        let expected_sentences = if strict && case.family == "E020" {
            let [start, end] = case.range.unwrap();
            let name = &case.source[start as usize..end as usize];
            vec![format!("Undeclared identifier '{name}' in equation. Fix: add '{name}' to a var, varexo, or parameters declaration.")]
        } else {
            case.sentences.clone()
        };
        let found = rows.iter().any(|row| {
            row.code == case.family
                && row.severity == severity
                && (!strict || expected_sentences.contains(&row.message))
                && (!strict
                    || case
                        .range
                        .is_none_or(|[start, end]| row.span.start == start && row.span.end == end))
        });
        if !found {
            failures.push(format!(
                "{}: missing {} {:?}, expected {:?} at {:?}: {rows:?}",
                case.name, case.family, severity, expected_sentences, case.range
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn installed_honesty_exact_model_expression_reach() {
    let Some(preprocessor) = pinned_binary() else {
        return;
    };
    for case in cases() {
        let result = run_preprocessor(
            &case.source,
            &preprocessor,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert_eq!(result.success, case.accepted, "{}: {result:?}", case.name);
        for sentence in &case.sentences {
            assert!(
                result.raw_stdout.contains(sentence),
                "{}: missing {sentence:?}: {result:?}",
                case.name
            );
        }
    }
}

fn official(source: &str, accepted: bool, sentence: Option<&str>) {
    let Some(preprocessor) = pinned_binary() else {
        return;
    };
    let result = run_preprocessor(
        source,
        &preprocessor,
        None,
        Duration::from_secs(30),
        JsonStage::Check,
    );
    assert_eq!(result.success, accepted, "{source}: {result:?}");
    if let Some(sentence) = sentence {
        assert!(result.raw_stdout.contains(sentence), "{source}: {result:?}");
    }
}

#[test]
fn model_roles_and_timing_use_the_type_known_before_later_retypes() {
    for (prefix, name, argument, code, sentence) in [
        ("external_function(name=bar,nargs=1);", "bar", "bar", "E280", "Symbol bar is a function name external to Dynare. It cannot be used like a variable without input argument inside model."),
        ("parameters p; p=local;", "local", "local", "E281", "Variable local not allowed inside model declaration. Its scope is only outside model."),
        ("var z; model; z=0; end; epilogue; epi=z; end;", "epi", "epi", "E294", "Symbol 'epi' cannot be used outside the epilogue block."),
        ("varexo_det ed;", "ed", "ed(-1)", "E024", "Exogenous deterministic variable ed cannot be given a lead or a lag."),
    ] {
        for before in [false, true] {
            // The earlier ordinary assignment has already used this implicit
            // local. Dynare forbids retyping it even before the model use.
            if before && code == "E281" {
                continue;
            }
            let change = format!("change_type(parameters) {name};");
            let source = format!("{prefix} {} external_function(name=fn,nargs=1); var y; model; y=fn({argument}); end; {}",
                if before { &change } else { "" }, if before { "" } else { &change });
            official(&source, before, if before { None } else { Some(sentence) });
            let rows = analyze(&parse(&source));
            assert_eq!(rows.iter().any(|row| row.code == code && row.message == sentence), !before, "{source}: {rows:?}");
            if before {
                assert!(!rows.iter().any(|row| row.severity == Severity::Error), "{source}: {rows:?}");
            }
        }
    }
}

#[test]
fn repeated_macro_spans_keep_each_model_expression_type_in_execution_order() {
    let source = "varexo_det ed; var y;\n@#for i in 1:2\n@#if i==2\nchange_type(parameters) ed;\n@#endif\nmodel; y=ed(-1); end;\n@#endfor\n";
    let sentence = "Exogenous deterministic variable ed cannot be given a lead or a lag.";
    official(source, false, Some(sentence));
    let rows = analyze(&parse(source));
    let rows: Vec<_> = rows.iter().filter(|row| row.code == "E024").collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].message, sentence);
    assert_eq!(
        &source[rows[0].span.start as usize..rows[0].span.end as usize],
        "ed(-1)"
    );

    // Only the first copy has the forbidden external-function role. The
    // second copy's type change cannot erase that earlier refusal.
    let source = "external_function(name=bar,nargs=1); var y;\n@#for i in 1:2\n@#if i==2\nchange_type(parameters) bar;\n@#endif\nmodel; y=bar; end;\n@#endfor\n";
    let sentence = "Symbol bar is a function name external to Dynare. It cannot be used like a variable without input argument inside model.";
    official(source, false, Some(sentence));
    let rows = analyze(&parse(source));
    let rows: Vec<_> = rows.iter().filter(|row| row.code == "E280").collect();
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].message, sentence);
    assert_eq!(
        &source[rows[0].span.start as usize..rows[0].span.end as usize],
        "bar"
    );
}

#[test]
fn integer_function_arguments_and_variable_timing_keep_distinct_trees() {
    for rhs in ["fn(1)", "pkg.fn(-1)", "exp(1)", "max(1,2)"] {
        let source = format!("external_function(name=fn,nargs=1); external_function(name=pkg.fn,nargs=1); var y; model; y={rhs}; end;");
        official(&source, true, None);
        let model = parse(&source);
        assert!(matches!(
            model.exprs.get(model.equations[0].rhs_expr.unwrap()).kind,
            ExprKind::Call { .. }
        ));
        assert!(!analyze(&model)
            .iter()
            .any(|row| row.severity == Severity::Error));
    }
    for rhs in ["p(-1)", "y(+1)", "ed(0)"] {
        let source = format!("parameters p; p=1; varexo_det ed; var y; model; y={rhs}; end;");
        official(&source, true, None);
        let model = parse(&source);
        assert!(matches!(
            model.exprs.get(model.equations[0].rhs_expr.unwrap()).kind,
            ExprKind::Ident {
                timing_span: Some(_),
                ..
            }
        ));
        assert!(!analyze(&model)
            .iter()
            .any(|row| row.severity == Severity::Error));
    }
}

#[test]
fn matched_moment_roles_use_prior_retypes_and_keep_their_row_context() {
    for source in [
        "varexo_det ed; change_type(var) ed; var y; model; y=0; ed=0; end; matched_moments; ed(-1); end;",
        "external_function(name=bar,nargs=1); change_type(var) bar; var y; model; y=0; bar=0; end; matched_moments; bar; end;",
    ] {
        official(source, true, None);
        let rows = analyze(&parse(source));
        assert!(!rows.iter().any(|row| row.severity == Severity::Error), "{source}: {rows:?}");
    }
    let source = "external_function(name=bar,nargs=1); var y; model; y=0; end;\n@#for i in 1:2\n@#if i==2\nchange_type(var) bar;\n@#endif\nmatched_moments; bar; end;\n@#endfor\n";
    let sentence = "Symbol bar is a function name external to Dynare. It cannot be used like a variable without input argument inside model.";
    official(source, false, Some(sentence));
    assert!(analyze(&parse(source))
        .iter()
        .any(|row| row.code == "E280" && row.message == sentence));
}

#[test]
fn replacement_unknown_arguments_keep_their_declaration_context() {
    let prefix = "external_function(name=fn,nargs=1); var y; model; [name='eq'] y=0; end;";
    for source in [
        format!("{prefix} model_replace('eq'); y=fn(missing); end; external_function(name=missing,nargs=1);"),
        format!("{prefix} model_replace('eq'); y=fn(missing); end; parameters p; p=missing;"),
        format!("{prefix}\n@#for i in 1:2\n@#if i==2\nparameters missing;\n@#endif\nmodel_replace('eq'); y=fn(missing); end;\n@#endfor\n"),
    ] {
        official(&source, false, Some("Unknown symbol: missing"));
        let rows = analyze(&parse(&source));
        let rows: Vec<_> = rows.iter().filter(|row| row.code == "E020").collect();
        assert_eq!(rows.len(), 1, "{source}: {rows:?}");
        assert_eq!(rows[0].severity, Severity::Error);
        assert_eq!(rows[0].message, "Undeclared identifier 'missing' in equation. Fix: add 'missing' to a var, varexo, or parameters declaration.");
        assert_eq!(rows[0].span.start as usize, source.find("fn(missing)").unwrap() + 3);
        assert_eq!(&source[rows[0].span.start as usize..rows[0].span.end as usize], "missing");
    }
    let source = format!("parameters missing; {prefix} model_replace('eq'); y=fn(missing); end;");
    official(&source, true, None);
    assert!(!analyze(&parse(&source))
        .iter()
        .any(|row| row.severity == Severity::Error));
}

#[test]
fn retyped_external_names_parse_as_variable_timing() {
    for (declaration, name, rhs) in [
        ("external_function(name=fn,nargs=1);", "fn", "fn(-1)"),
        (
            "external_function(name=fn,nargs=1); external_function(name=bar,nargs=1);",
            "bar",
            "fn(bar(-1))",
        ),
        (
            "external_function(name=fn,nargs=1,first_deriv_provided=jac);",
            "jac",
            "fn(jac(-1))",
        ),
    ] {
        let source =
            format!("{declaration} change_type(varexo_det) {name}; var y; model; y={rhs}; end;");
        let sentence =
            format!("Exogenous deterministic variable {name} cannot be given a lead or a lag.");
        official(&source, false, Some(&sentence));
        let model = parse(&source);
        let rhs = model.equations[0].rhs_expr.unwrap();
        let timed = if name == "fn" {
            rhs
        } else {
            let ExprKind::Call { args, .. } = &model.exprs.get(rhs).kind else {
                panic!("Expected the outer fn call: {source}");
            };
            args[0]
        };
        assert!(matches!(
            model.exprs.get(timed).kind,
            ExprKind::Ident {
                timing: -1,
                timing_span: Some(_),
                ..
            }
        ));
        let rows = analyze(&model);
        let row = rows
            .iter()
            .find(|row| row.code == "E024" && row.message == sentence)
            .unwrap_or_else(|| panic!("{source}: {rows:?}"));
        assert_eq!(row.severity, Severity::Error);
        assert_eq!(
            &source[row.span.start as usize..row.span.end as usize],
            format!("{name}(-1)")
        );
    }
    let source = "external_function(name=fn,nargs=1); change_type(var) fn; var y; model; y=fn(1); fn=0; end;";
    official(source, true, None);
    let model = parse(source);
    assert!(matches!(
        model.exprs.get(model.equations[0].rhs_expr.unwrap()).kind,
        ExprKind::Ident {
            timing: 1,
            timing_span: Some(_),
            ..
        }
    ));
    assert!(!analyze(&model)
        .iter()
        .any(|row| row.severity == Severity::Error));
}
