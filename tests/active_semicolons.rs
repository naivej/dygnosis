//! Active equation syntax for semicolon, merged-assignment, and parenthesis scans.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dygnosis::server::new_service;
use dygnosis::{
    analyze, apply_fix, auto_fix, check_file, check_parse, dynare_diagnose, dynare_expand, parse,
    run_preprocessor, ExprId, ExprKind, JsonStage, Model,
};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

const MERGED: &str = "merged due to a missing semicolon";
const UNBALANCED: &str = "Unbalanced parentheses";

fn messages(text: &str) -> Vec<String> {
    check_parse(&parse(text))
        .into_iter()
        .map(|diag| diag.message)
        .collect()
}

fn has_needle(text: &str, needle: &str) -> bool {
    messages(text)
        .iter()
        .any(|message| message.contains(needle))
}

fn codes(text: &str) -> Vec<String> {
    analyze(&parse(text))
        .into_iter()
        .map(|diag| diag.code)
        .collect()
}

fn false_witness() -> &'static str {
    "var y;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#else\ny=0;\n@#endif\nend;\n"
}

#[test]
fn tagged_discarded_branch_is_not_a_second_equation() {
    let text = false_witness();
    assert!(
        !has_needle(text, MERGED),
        "false semicolon: {}",
        messages(text).join(" | ")
    );
    assert!(
        !has_needle(text, UNBALANCED),
        "{}",
        messages(text).join(" | ")
    );
    let expanded = dynare_expand(text, None, None);
    assert_eq!(expanded["complete"], true, "{expanded}");
    assert_eq!(expanded["n_equations"], 1, "{expanded}");
    assert_eq!(auto_fix(text), text);
}

#[test]
fn semicolon_controls_keep_real_reports_and_drop_discarded_ones() {
    let quiet = [
        "var y;\nmodel;\n[name='law']\n@#if 1\ny=0;\n@#else\ny=0\nz=0;\n@#endif\nend;\n",
        "var y;\nmodel;\n@#if 0\ny=0;\n@#else\ny=0;\n@#endif\nend;\n",
        "var y;\nmodel;\n@#if 0\n[name='other']\ny=0;\n@#else\n[name='law']\ny=0;\n@#endif\nend;\n",
        "var y;\nmodel;\n[name='law']\n// @#if 0\n// y=0;\n// @#else\ny=0;\n// @#endif\nend;\n",
        "var y;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#elseif 0\ny=1;\n@#else\ny=0;\n@#endif\nend;\n",
        "var y;\nmodel;\n[name='law']\n@#if 1\n@#if 0\ny=0;\n@#else\ny=0;\n@#endif\n@#else\ny=1;\n@#endif\nend;\n",
        "var y z;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#else\ny=0;\nz=0;\n@#endif\nend;\n",
    ];
    for text in quiet {
        assert!(
            !has_needle(text, MERGED),
            "{text}\n{}",
            messages(text).join(" | ")
        );
    }

    let missing = "var y;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#else\ny=0\n@#endif\nend;\n";
    let missing_diags = check_parse(&parse(missing));
    assert!(
        missing_diags.iter().any(|diag| {
            diag.code == "E001" && diag.fix.is_some() && diag.message.contains("semicolon")
        }),
        "{missing_diags:?}"
    );
    let fixed = auto_fix(missing);
    assert_ne!(fixed, missing);
    assert!(
        fixed.contains("@#if 0\ny=0;\n@#else"),
        "discarded branch changed: {fixed}"
    );
    assert!(
        !check_parse(&parse(&fixed))
            .iter()
            .any(|diag| diag.message.contains("missing its terminating semicolon")),
        "{fixed}\n{:?}",
        check_parse(&parse(&fixed))
    );

    let merged = "var y z;\nmodel;\ny=0\nz=0;\nend;\n";
    assert!(
        has_needle(merged, MERGED),
        "{}",
        messages(merged).join(" | ")
    );
    assert!(!has_needle(&auto_fix(merged), MERGED));
}

#[test]
fn assignment_and_parenthesis_scans_use_the_same_active_tokens() {
    let false_assignment =
        "parameters beta alpha;\nbeta = @#if 0\n1\nalpha = 2\n@#else\n1 + alpha = 2;\n@#endif\n";
    assert!(
        has_needle(false_assignment, MERGED),
        "active merged assignment must still report: {}",
        messages(false_assignment).join(" | ")
    );
    let discarded_only = "parameters beta;\nbeta = @#if 0\n1\nalpha = 2\n@#else\n0.99;\n@#endif\n";
    assert!(
        !has_needle(discarded_only, MERGED),
        "{}",
        messages(discarded_only).join(" | ")
    );
    let bracket_would_suppress =
        "parameters beta alpha;\nbeta = @#if 0\n[1]\nalpha = 2\n@#else\n1 + alpha = 2;\n@#endif\n";
    assert!(
        has_needle(bracket_would_suppress, MERGED),
        "discarded brackets must not hide the active merge: {}",
        messages(bracket_would_suppress).join(" | ")
    );

    let false_paren =
        "var y;\nmodel;\n[name='law']\n@#if 0\ny = (1;\n@#else\ny = 0;\n@#endif\nend;\n";
    assert!(
        !has_needle(false_paren, UNBALANCED),
        "{}",
        messages(false_paren).join(" | ")
    );
    let real_paren = "var y;\nmodel;\ny = (1;\nend;\n";
    assert!(
        has_needle(real_paren, UNBALANCED),
        "{}",
        messages(real_paren).join(" | ")
    );
}

#[test]
fn repeated_executions_report_only_the_selected_merge() {
    let text = "\
var y z;
model;
@#for i in 1:2
[name='row']
@#if i == 1
y = 0;
@#else
y = 0
z = 0;
@#endif
@#endfor
end;
";
    let hits: Vec<_> = messages(text)
        .into_iter()
        .filter(|message| message.contains(MERGED))
        .collect();
    assert_eq!(hits.len(), 1, "{hits:?}");
}

#[test]
fn unknown_branch_is_not_guessed() {
    let text = "\
var y z;
model;
@#if unknown_name
y = (1;
@#else
y = 0
z = 0;
@#endif
end;
";
    let got = messages(text);
    assert!(
        got.iter()
            .all(|message| !message.contains(MERGED) && !message.contains(UNBALANCED)),
        "{got:?}"
    );
    assert!(
        codes(text).iter().any(|code| code == "E063"),
        "{:?}",
        codes(text)
    );
}

#[test]
fn reach_discarded_names_stay_quiet_and_active_names_still_fire() {
    let discarded = "\
var y;
model;
[name='law']
@#if 0
y = zz;
@#else
y = 0;
@#endif
end;
";
    let discarded_codes = codes(discarded);
    assert!(
        discarded_codes
            .iter()
            .all(|code| code != "E020" && code != "E030" && code != "E001"),
        "{discarded_codes:?}"
    );
    let active = "var y; model; y = zz; end;";
    assert!(
        codes(active).iter().any(|code| code == "E020"),
        "{:?}",
        codes(active)
    );
    let inactive_dup = "\
@#if 0
var y;
varexo y;
@#else
var y;
@#endif
model;
y = 0;
end;
";
    assert!(
        codes(inactive_dup).iter().all(|code| code != "E030"),
        "{:?}",
        codes(inactive_dup)
    );
    let active_dup = "var y; varexo y; model; y = 0; end;";
    assert!(
        codes(active_dup).iter().any(|code| code == "E030"),
        "{:?}",
        codes(active_dup)
    );
}

#[test]
fn included_tagged_branch_does_not_merge() {
    let dir = std::env::temp_dir().join(format!(
        "dygnosis-active-semi-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    let parent = dir.join("parent.mod");
    let child = dir.join("child.inc");
    fs::write(
        &child,
        "[name='law']\n@#if 0\ny = 0;\n@#else\ny = 0;\n@#endif\n",
    )
    .unwrap();
    let text = "var y;\nmodel;\n@#include \"child.inc\"\nend;\n";
    fs::write(&parent, text).unwrap();
    let key = parent.to_string_lossy().to_string();
    let diags = check_file(text, &key);
    assert!(
        diags.iter().all(|diag| !diag.message.contains(MERGED)),
        "{diags:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn archive_text() -> (PathBuf, String, String) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/use-dynare/references/examples/NK_CDK24_RANK_rep.mod");
    let raw =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
    let text = raw.replace("\r\n", "\n");
    assert!(
        text.contains("@#include \"set_parameters.m\""),
        "archive include line moved"
    );
    let disabled = text.replacen(
        "@#include \"set_parameters.m\"",
        "// @#include \"set_parameters.m\"",
        1,
    );
    assert!(disabled.contains("// @#include \"set_parameters.m\""));
    assert!(!disabled
        .lines()
        .any(|line| line.trim_start() == "@#include \"set_parameters.m\""));
    (path, text, disabled)
}

fn no_merged(diags: &[(String, String)]) {
    assert!(
        diags
            .iter()
            .all(|(code, message)| { !(code == "E001" && message.contains(MERGED)) }),
        "{diags:?}"
    );
}

#[test]
fn archive_include_disabled_has_no_merged_semicolon() {
    let (path, original, disabled) = archive_text();
    assert_ne!(original, disabled);
    let key = path.to_string_lossy().to_string();
    let library: Vec<_> = check_file(&disabled, &key)
        .into_iter()
        .map(|diag| (diag.code, diag.message))
        .collect();
    no_merged(&library);
    let parsed: Vec<_> = analyze(&parse(&disabled))
        .into_iter()
        .map(|diag| (diag.code, diag.message))
        .collect();
    no_merged(&parsed);
    let files = HashMap::from([(key.clone(), disabled.clone())]);
    let mcp: Vec<_> = dynare_diagnose(&disabled, Some(&key), Some(&files))
        .into_iter()
        .map(|diag| (diag.code, diag.message))
        .collect();
    no_merged(&mcp);
    assert!(
        library.iter().all(|(code, _)| code != "E061"),
        "commented include must not be a missing file: {library:?}"
    );
}

#[tokio::test]
async fn archive_lsp_include_disabled_has_no_merged_semicolon() {
    let (path, _, disabled) = archive_text();
    let uri = Url::from_file_path(&path).unwrap();
    let (service, _socket) = new_service();
    service
        .inner()
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: disabled,
            },
        })
        .await;
    let report = service
        .inner()
        .diagnostic(DocumentDiagnosticParams {
            text_document: TextDocumentIdentifier { uri },
            identifier: None,
            previous_result_id: None,
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
        .await
        .expect("pull");
    let DocumentDiagnosticReportResult::Report(DocumentDiagnosticReport::Full(full)) = report
    else {
        panic!("expected a full report");
    };
    assert!(full
        .full_document_diagnostic_report
        .items
        .iter()
        .all(|diag| !diag.message.contains(MERGED)));
}

fn expr_has_error(model: &Model, id: ExprId) -> bool {
    match &model.exprs.get(id).kind {
        ExprKind::Error => true,
        ExprKind::Unary { arg, .. } => expr_has_error(model, *arg),
        ExprKind::Binary { lhs, rhs, .. } => {
            expr_has_error(model, *lhs) || expr_has_error(model, *rhs)
        }
        ExprKind::Call { args, .. } => args.iter().any(|arg| expr_has_error(model, *arg)),
        ExprKind::SteadyState { arg } | ExprKind::Expectation { arg, .. } => {
            expr_has_error(model, *arg)
        }
        ExprKind::Ident { .. } | ExprKind::Number | ExprKind::String => false,
    }
}

fn equations_are_complete(text: &str) {
    let model = parse(text);
    assert!(!model.equations.is_empty(), "{text}");
    for equation in &model.equations {
        let lhs = equation.lhs_expr.expect("lhs");
        let rhs = equation.rhs_expr.expect("rhs");
        assert!(!expr_has_error(&model, lhs), "{}", equation.text);
        assert!(!expr_has_error(&model, rhs), "{}", equation.text);
    }
}

fn assignment_rhs_is_complete(text: &str, name: &str) {
    let model = parse(text);
    let assignment = model
        .param_assignments
        .iter()
        .find(|assignment| model.intern.get(assignment.name) == name)
        .unwrap_or_else(|| panic!("missing {name}: {text}"));
    let expr = assignment.expr.expect("rhs");
    assert!(
        model
            .assignment_syntax
            .get(&expr)
            .is_some_and(|syntax| syntax.full_rhs),
        "{}",
        assignment.expression
    );
    assert!(!expr_has_error(&model, expr), "{}", assignment.expression);
}

fn merged_diag(text: &str) -> dygnosis::Diagnostic {
    check_parse(&parse(text))
        .into_iter()
        .find(|diag| diag.message.contains(MERGED))
        .unwrap_or_else(|| panic!("missing merge: {text}\n{}", messages(text).join(" | ")))
}

/// Apply the stored edit and require the library owner gate to accept that same edit.
fn repaired(text: &str) -> String {
    let diag = merged_diag(text);
    let fix = diag
        .fix
        .clone()
        .unwrap_or_else(|| panic!("unproved separator: {}", diag.message));
    let edited = apply_fix(text, std::slice::from_ref(&fix));
    assert_eq!(
        auto_fix(text),
        edited,
        "owner gate did not apply the stored separator"
    );
    assert!(!has_needle(&edited, MERGED), "{edited}");
    edited
}

#[test]
fn merged_separator_uses_the_active_token_not_the_previous_raw_line() {
    let dormant = "var y z;\nmodel;\ny=0\n@#if 0\ny=1;\n@#endif\nz=0;\nend;\n";
    let dormant_fixed = repaired(dormant);
    assert_eq!(
        dormant_fixed,
        "var y z;\nmodel;\ny=0;\n@#if 0\ny=1;\n@#endif\nz=0;\nend;\n"
    );

    let commented = "var y z;\nmodel;\ny=0 // keep\n@#if 0\ny=1;\n@#endif\nz=0;\nend;\n";
    let commented_fixed = repaired(commented);
    assert!(
        commented_fixed.contains("y=0; // keep\n@#if 0\ny=1;\n@#endif\n"),
        "{commented_fixed}"
    );

    let selected = "var y z;\nmodel;\n@#if 1\ny=0\n@#endif\nz=0;\nend;\n";
    let selected_fixed = repaired(selected);
    assert_eq!(
        selected_fixed,
        "var y z;\nmodel;\n@#if 1\ny=0;\n@#endif\nz=0;\nend;\n"
    );

    let same_line = "var y z;\nmodel;\ny=0 z=0;\nend;\n";
    let same = merged_diag(same_line);
    assert_eq!(
        same.fix.as_ref().map(|fix| fix.new_text.as_str()),
        Some(";\n")
    );
    let same_fixed = repaired(same_line);
    assert!(same_fixed.contains("y=0 ;\nz=0;"), "{same_fixed}");

    let copied = "\
var y z;
model;
@#for i in 1:2
[name='row']
@#if i == 1
y = 0;
@#else
y = 0
z = 0;
@#endif
@#endfor
end;
";
    let copied_fixed = repaired(copied);
    assert!(
        copied_fixed.contains("@#else\ny = 0;\nz = 0;"),
        "{copied_fixed}"
    );
    assert!(
        copied_fixed.contains("@#if i == 1\ny = 0;"),
        "{copied_fixed}"
    );
    assert!(!copied_fixed.contains("@#endif;"), "{copied_fixed}");
    assert!(!copied_fixed.contains("@#else;"), "{copied_fixed}");

    let generated = "@#define n = 1\nvar y z;\nmodel;\ny=@{n}\nz=0;\nend;\n";
    let generated_diag = merged_diag(generated);
    let generated_fix = generated_diag.fix.clone().unwrap_or_else(|| {
        panic!(
            "unproved interpolation boundary: {}",
            generated_diag.message
        )
    });
    let generated_edited = apply_fix(generated, std::slice::from_ref(&generated_fix));
    assert_eq!(
        generated_edited,
        "@#define n = 1\nvar y z;\nmodel;\ny=@{n};\nz=0;\nend;\n"
    );
    assert!(
        !has_needle(&generated_edited, MERGED),
        "single-token edit must repair its owner: {generated_edited}"
    );
    equations_are_complete(&generated_edited);
    assert_eq!(
        auto_fix(generated),
        generated,
        "define plus interpolation stays refused"
    );

    let split_generated = "@#define gen = \"0 z\"\nvar y z;\nmodel;\ny = @{gen} = 0;\nend;\n";
    let split_diag = merged_diag(split_generated);
    assert!(
        split_diag.fix.is_none(),
        "shared interpolation span is not a separator: {:?}",
        split_diag.fix
    );
    assert!(
        !split_diag.message.contains("Fix:"),
        "{}",
        split_diag.message
    );
    assert_eq!(auto_fix(split_generated), split_generated);

    let trailing_plus = "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny = 1 + @{gen};\nend;\n";
    let trailing_plus_diag = merged_diag(trailing_plus);
    assert!(
        trailing_plus_diag.fix.is_none(),
        "{:?}",
        trailing_plus_diag.fix
    );
    assert!(!trailing_plus_diag.message.contains("Fix:"));
    assert_eq!(auto_fix(trailing_plus), trailing_plus);

    let trailing_anchor = "@#define gen = \"1 +\"\nvar y z;\nmodel;\ny = @{gen}\nz = 0;\nend;\n";
    let trailing_anchor_diag = merged_diag(trailing_anchor);
    assert!(
        trailing_anchor_diag.fix.is_none(),
        "{:?}",
        trailing_anchor_diag.fix
    );
    assert!(!trailing_anchor_diag.message.contains("Fix:"));
    assert_eq!(auto_fix(trailing_anchor), trailing_anchor);

    let dangling_assignment =
        "@#define gen = \"alpha = 2\"\nparameters beta alpha;\nbeta = 1 + @{gen};\n";
    let dangling_diag = merged_diag(dangling_assignment);
    assert!(dangling_diag.fix.is_none(), "{:?}", dangling_diag.fix);
    assert!(!dangling_diag.message.contains("Fix:"));
    assert_eq!(auto_fix(dangling_assignment), dangling_assignment);

    let before_name = "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny=1 @{gen};\nend;\n";
    let before_fix = merged_diag(before_name)
        .fix
        .expect("complete value before the generated equation");
    let before_fixed = apply_fix(before_name, std::slice::from_ref(&before_fix));
    assert_eq!(
        before_fixed, "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny=1 ;\n@{gen};\nend;\n",
        "{before_fixed}"
    );
    equations_are_complete(&before_fixed);
    let before_model = parse(&before_fixed);
    assert_eq!(before_model.equations[0].lhs, "y");
    assert_eq!(before_model.equations[0].rhs, "1");
    assert_eq!(before_model.equations[1].lhs, "z");
    assert_eq!(before_model.equations[1].rhs, "0");

    let after_value = "@#define gen = \"1 + 0\"\nvar y z;\nmodel;\ny=@{gen}\nz=0;\nend;\n";
    let after_fix = merged_diag(after_value)
        .fix
        .expect("complete value at the end of the interpolation");
    let after_fixed = apply_fix(after_value, std::slice::from_ref(&after_fix));
    assert_eq!(
        after_fixed, "@#define gen = \"1 + 0\"\nvar y z;\nmodel;\ny=@{gen};\nz=0;\nend;\n",
        "{after_fixed}"
    );
    equations_are_complete(&after_fixed);
    let after_model = parse(&after_fixed);
    assert_eq!(after_model.equations[0].rhs, "1+0");
    assert_eq!(after_model.equations[1].lhs, "z");
    assert_eq!(after_model.equations[1].rhs, "0");

    let branched = "\
@#define gen = \"1 + 0\"
var y z;
model;
@#if 1
y=@{gen}
@#else
y=9;
@#endif
z=0;
end;
";
    let branched_fix = merged_diag(branched).fix.expect("selected branch");
    let branched_fixed = apply_fix(branched, std::slice::from_ref(&branched_fix));
    assert!(
        branched_fixed.contains("@#if 1\ny=@{gen};\n@#else\ny=9;\n@#endif\nz=0;"),
        "{branched_fixed}"
    );
    assert!(!branched_fixed.contains("@#endif;"), "{branched_fixed}");
    assert!(!branched_fixed.contains("@#else;"), "{branched_fixed}");
    equations_are_complete(&branched_fixed);

    let physical_assignment = "parameters beta alpha;\nbeta = 1\nalpha = 2;\n";
    let physical_diag = check_parse(&parse(physical_assignment))
        .into_iter()
        .find(|diag| diag.message.contains("missing its terminating semicolon"))
        .expect("owning missing-semicolon diagnostic");
    let physical_fix = physical_diag.fix.expect("complete assignment stores ';'");
    let physical_fixed = apply_fix(physical_assignment, std::slice::from_ref(&physical_fix));
    assert_eq!(
        physical_fixed,
        "parameters beta alpha;\nbeta = 1;\nalpha = 2;\n"
    );
    assert_eq!(auto_fix(physical_assignment), physical_fixed);
    assignment_rhs_is_complete(&physical_fixed, "beta");
    assignment_rhs_is_complete(&physical_fixed, "alpha");

    let unfinished_assignment =
        "@#define gen = \"1 +\"\nparameters beta alpha;\nbeta = @{gen}\nalpha = 2;\n";
    let unfinished = check_parse(&parse(unfinished_assignment));
    assert!(
        unfinished.iter().all(|diag| diag.fix.is_none()),
        "{unfinished:?}"
    );
    assert!(unfinished.iter().all(|diag| !diag.message.contains("Fix:")));
    assert_eq!(auto_fix(unfinished_assignment), unfinished_assignment);

    let assignment = "\
parameters beta alpha;
beta = @#if 0
1
alpha = 2
@#else
1 + alpha = 2;
@#endif
";
    let assignment_diag = merged_diag(assignment);
    assert!(assignment_diag.fix.is_none(), "{:?}", assignment_diag.fix);
    assert!(!assignment_diag.message.contains("Fix:"));
    assert_eq!(auto_fix(assignment), assignment);
    assert!(assignment.contains("@#if 0\n1\nalpha = 2\n@#else\n"));
    assert!(assignment.contains("@#endif"));
    assert!(!assignment.contains("@#endif;"));
}

/// Pinned Dynare 7.2. Another install or `DYNARE_PREPROCESSOR` is not used.
fn pinned_dynare_72() -> Option<PathBuf> {
    let path = PathBuf::from("C:/dynare/7.2/preprocessor/dynare-preprocessor.exe");
    assert!(
        path.components().any(|part| part.as_os_str() == "7.2"),
        "honesty pin must be Dynare 7.2"
    );
    path.is_file().then_some(path)
}

#[test]
fn honesty_at_check_for_the_semicolon_repairs() {
    let Some(preprocessor) = pinned_dynare_72() else {
        eprintln!("skipping honesty: Dynare 7.2 is absent");
        return;
    };
    let accepted = false_witness();
    let missing = "var y;\nmodel;\n[name='law']\n@#if 0\ny=0;\n@#else\ny=0\n@#endif\nend;\n";
    let merged = "var y z;\nmodel;\ny=0\nz=0;\nend;\n";
    let (_, _, archive) = archive_text();
    for (label, text, expect_ok, needle) in [
        ("witness", accepted, true, None),
        ("missing", missing, false, Some("unexpected END")),
        ("merged", merged, false, Some("unexpected IDENTIFIER")),
        ("archive", archive.as_str(), true, None),
    ] {
        let result = run_preprocessor(
            text,
            &preprocessor,
            None,
            Duration::from_secs(120),
            JsonStage::Check,
        );
        let combined = format!(
            "{}\n{}\n{}",
            result.raw_stdout,
            result.raw_stderr,
            result
                .diagnostics
                .iter()
                .map(|diag| diag.message.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert_eq!(
            result.success,
            expect_ok,
            "{label} exit {:?} severity {:?}\n{combined}",
            result.exit_code,
            result
                .diagnostics
                .iter()
                .map(|diag| diag.severity)
                .collect::<Vec<_>>()
        );
        if let Some(needle) = needle {
            assert!(
                combined.contains(needle),
                "{label} missing {needle}: {combined}"
            );
        }
    }
}

#[test]
fn grouping_is_one_expression_and_calls_keep_arguments() {
    for text in [
        "var y z;\nmodel;\ny=() z=0;\nend;\n",
        "var y z;\nmodel;\ny=(1,2) z=0;\nend;\n",
        "@#define gen = \"()\"\nvar y z;\nmodel;\ny=@{gen} z=0;\nend;\n",
        "@#define gen = \"(1,2)\"\nvar y z;\nmodel;\ny=@{gen} z=0;\nend;\n",
    ] {
        let diag = merged_diag(text);
        assert!(diag.fix.is_none(), "{text}\n{:?}", diag.fix);
        assert!(!diag.message.contains("Fix:"), "{text}\n{}", diag.message);
        assert_eq!(auto_fix(text), text, "{text}");
    }

    let scalar = "var y z;\nmodel;\ny=(1) z=0;\nend;\n";
    let scalar_fixed = repaired(scalar);
    assert_eq!(scalar_fixed, "var y z;\nmodel;\ny=(1) ;\nz=0;\nend;\n");
    equations_are_complete(&scalar_fixed);
    let scalar_model = parse(&scalar_fixed);
    assert_eq!(scalar_model.equations[0].lhs, "y");
    assert_eq!(scalar_model.equations[0].rhs, "(1)");
    assert_eq!(scalar_model.equations[1].lhs, "z");
    assert_eq!(scalar_model.equations[1].rhs, "0");

    let call = "var y z;\nmodel;\ny=max(1,2) z=0;\nend;\n";
    let call_fixed = repaired(call);
    assert_eq!(call_fixed, "var y z;\nmodel;\ny=max(1,2) ;\nz=0;\nend;\n");
    equations_are_complete(&call_fixed);
    let call_model = parse(&call_fixed);
    assert_eq!(call_model.equations[0].rhs, "max(1, 2)");
    assert_eq!(call_model.equations[1].rhs, "0");

    let timed = "var y z;\nmodel;\ny=y(-1) z=0;\nend;\n";
    let timed_fixed = repaired(timed);
    assert_eq!(timed_fixed, "var y z;\nmodel;\ny=y(-1) ;\nz=0;\nend;\n");
    equations_are_complete(&timed_fixed);

    let nested = "var y z;\nmodel;\ny=max(min(1,2),3) z=0;\nend;\n";
    let nested_fixed = repaired(nested);
    assert_eq!(
        nested_fixed,
        "var y z;\nmodel;\ny=max(min(1,2),3) ;\nz=0;\nend;\n"
    );
    equations_are_complete(&nested_fixed);
}

#[test]
fn complete_separator_repairs_agree_with_pinned_check() {
    let Some(preprocessor) = pinned_dynare_72() else {
        eprintln!("skipping honesty: Dynare 7.2 is absent");
        return;
    };
    let accepted = [
        "@#define gen = \"z = 0\"\nvar y z;\nmodel;\ny=1 ;\n@{gen};\nend;\n",
        "@#define gen = \"1 + 0\"\nvar y z;\nmodel;\ny=@{gen};\nz=0;\nend;\n",
        "@#define n = 1\nvar y z;\nmodel;\ny=@{n};\nz=0;\nend;\n",
        "parameters beta alpha;\nbeta = 1;\nalpha = 2;\n",
    ];
    for text in accepted {
        let result = run_preprocessor(
            text,
            &preprocessor,
            None,
            Duration::from_secs(120),
            JsonStage::Check,
        );
        let combined = format!("{}\n{}", result.raw_stdout, result.raw_stderr);
        assert!(result.success, "exit {:?}\n{combined}", result.exit_code);
        assert!(
            !combined.to_ascii_lowercase().contains("syntax error"),
            "{combined}"
        );
    }
}

#[test]
fn grouping_edits_agree_with_pinned_check() {
    let Some(preprocessor) = pinned_dynare_72() else {
        eprintln!("skipping honesty: Dynare 7.2 is absent");
        return;
    };
    let refused = [
        (
            "empty group",
            "var y z;\nmodel;\ny=();\nz=0;\nend;\n",
            "unexpected ')'",
        ),
        (
            "group comma",
            "var y z;\nmodel;\ny=(1,2);\nz=0;\nend;\n",
            "unexpected COMMA",
        ),
        (
            "generated empty",
            "@#define gen = \"()\"\nvar y z;\nmodel;\ny=@{gen};\nz=0;\nend;\n",
            "unexpected ')'",
        ),
        (
            "generated comma",
            "@#define gen = \"(1,2)\"\nvar y z;\nmodel;\ny=@{gen};\nz=0;\nend;\n",
            "unexpected COMMA",
        ),
    ];
    let accepted = [
        "var y z;\nmodel;\ny=(1);\nz=0;\nend;\n",
        "var y z;\nmodel;\ny=max(1,2);\nz=0;\nend;\n",
        "var y z;\nmodel;\ny=y(-1);\nz=0;\nend;\n",
        "var y z;\nmodel;\ny=max(min(1,2),3);\nz=0;\nend;\n",
    ];
    for (label, text, needle) in refused {
        let result = run_preprocessor(
            text,
            &preprocessor,
            None,
            Duration::from_secs(120),
            JsonStage::Check,
        );
        let combined = format!(
            "{}\n{}\n{}",
            result.raw_stdout,
            result.raw_stderr,
            result
                .diagnostics
                .iter()
                .map(|diag| diag.message.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        assert!(!result.success, "{label} was accepted\n{combined}");
        assert!(
            combined.contains(needle),
            "{label} missing {needle}: {combined}"
        );
    }
    for text in accepted {
        let result = run_preprocessor(
            text,
            &preprocessor,
            None,
            Duration::from_secs(120),
            JsonStage::Check,
        );
        let combined = format!("{}\n{}", result.raw_stdout, result.raw_stderr);
        assert!(
            result.success,
            "{text}\nexit {:?}\n{combined}",
            result.exit_code
        );
        assert!(
            !combined.to_ascii_lowercase().contains("syntax error"),
            "{combined}"
        );
    }
}
