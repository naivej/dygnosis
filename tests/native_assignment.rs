//! Native MATLAB assignments stop at Dynare's `NATIVE` end.
//!
//! An unknown name, a mod-file local, and an external function skip the
//! expression. A `#` local and a `model_local_variable` stay expressions.

use std::time::Duration;

use dygnosis::server::diagnostics_for;
use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, Diagnostic, JsonStage,
};
use tower_lsp::lsp_types::NumberOrString;

fn diags(source: &str) -> Vec<Diagnostic> {
    analyze(&parse(source))
}

fn has(rows: &[Diagnostic], code: &str) -> bool {
    rows.iter().any(|row| row.code == code)
}

fn quiet(source: &str, code: &str) {
    let rows = diags(source);
    assert!(!has(&rows, code), "{source}: {rows:?}");
}

fn message<'a>(rows: &'a [Diagnostic], code: &str) -> &'a str {
    rows.iter()
        .find(|row| row.code == code)
        .unwrap_or_else(|| panic!("expected {code}, got {rows:?}"))
        .message
        .as_str()
}

const NAMESPACE: &str = "Namespace-qualified symbol pp.rho not allowed in this context";

fn agree(source: &str, code: &str) {
    let library = diags(source);
    let want = has(&library, code);
    let mcp = dynare_diagnose(source, None, None);
    assert_eq!(mcp.iter().any(|row| row.code == code), want, "{mcp:?}");
    let lsp = diagnostics_for("file:///C:/tmp/native_assignment.mod", source);
    assert_eq!(
        lsp.iter().any(|row| {
            matches!(&row.code, Some(NumberOrString::String(found)) if found == code)
        }),
        want,
        "{lsp:?}"
    );
    if want {
        let text = message(&library, code);
        assert_eq!(
            mcp.iter().find(|row| row.code == code).unwrap().message,
            text
        );
        let lsp_row = lsp
            .iter()
            .find(|row| matches!(&row.code, Some(NumberOrString::String(found)) if found == code))
            .unwrap();
        assert_eq!(lsp_row.message, text);
        let mcp_row = mcp.iter().find(|row| row.code == code).unwrap();
        assert_eq!(mcp_row.line, lsp_row.range.start.line + 1);
        assert_eq!(mcp_row.column, lsp_row.range.start.character + 1);
    }
}

#[test]
fn archive_line_and_native_witness_have_no_e275() {
    let archive = "\
parameters y_clean;
y_clean = 1;
steady_state_proof=100*abs(y_clean-oo_.steady_state(8))
";
    quiet(archive, "E275");
    agree(archive, "E275");
    let witness = "\
parameters rho;
rho = 0.9;
proof = oo_.steady_state(8);
proof = M_.params(1);
proof = pp.rho;
";
    quiet(witness, "E275");
    agree(witness, "E275");
}

#[test]
fn the_same_text_on_the_next_line_still_reports_e275() {
    let next = "\
parameters rho;
rho = 0.9;
proof = 1;
rho = pp.rho;
";
    let rows = diags(next);
    assert_eq!(message(&rows, "E275"), NAMESPACE);
    assert!(!has(&rows, "W011"), "{rows:?}");
    agree(next, "E275");
    let same = "\
parameters rho;
rho = 0.9;
proof = 1; rho = pp.rho;
";
    quiet(same, "E275");
    agree(same, "E275");
}

#[test]
fn declared_namespace_and_steady_state_assignments_stay_refused() {
    for source in [
        "parameters rho; rho = pp.rho;",
        "parameters y_clean; y_clean = oo_.steady_state;",
        "parameters y_clean; y_clean = oo_.steady_state(8);",
    ] {
        assert!(has(&diags(source), "E275"), "{source}");
        agree(source, "E275");
    }
}

#[test]
fn native_line_end_follows_the_pin() {
    let continued = "parameters rho; rho = 0.9;\nproof = 1 ...\nrho = pp.rho;\n";
    quiet(continued, "E275");
    let blank = "parameters rho; rho = 0.9;\nproof = 1 ...\n\nrho = pp.rho;\n";
    quiet(blank, "E275");
    let inside = "parameters rho; rho = 0.9;\nproof = 1; /* keep\ngoing */ rho = pp.rho;\n";
    quiet(inside, "E275");
    let closed = "parameters rho; rho = 0.9;\nproof = 1; /*\n*/\nrho = pp.rho;\n";
    assert_eq!(message(&diags(closed), "E275"), NAMESPACE);
    let percent = "parameters rho; rho = 0.9;\nproof = 1; % rho = pp.rho;\nrho = pp.rho;\n";
    assert_eq!(message(&diags(percent), "E275"), NAMESPACE);
    let quoted = "parameters rho; rho = 0.9;\nproof = '/* not';\nrho = pp.rho;\n";
    assert_eq!(message(&diags(quoted), "E275"), NAMESPACE);
    let native_comment = "parameters rho; rho = 0.9;\nproof = 1 ... /*\n*/\nrho = pp.rho;\n";
    quiet(native_comment, "E275");
}

#[test]
fn a_native_line_keeps_later_commands_and_the_next_line_parses_them() {
    let same_data = "parameters rho; rho = 0.9;\nzz = 1; data(nobs=1);\n";
    assert!(parse(same_data).data_statements.is_empty(), "{same_data}");
    quiet(same_data, "E001");
    let two = "parameters rho; rho = 0.9;\nzz = 1; zz2 = 2; data(nobs=1);\n";
    assert!(parse(two).data_statements.is_empty(), "{two}");
    quiet(two, "E001");
    let next_data = "parameters rho; rho = 0.9;\nzz = 1;\ndata(nobs=1);\n";
    assert_eq!(parse(next_data).data_statements.len(), 1);
    let same_shocks = "parameters rho; rho = 0.9;\nzz = 1; shocks;\n";
    assert!(parse(same_shocks).shocks_block.is_none(), "{same_shocks}");
    let next_shocks = "parameters rho; rho = 0.9;\nzz = 1;\nshocks;\n";
    assert!(parse(next_shocks).shocks_block.is_some());
}

#[test]
fn three_native_heads_skip_the_expression() {
    let unknown = "proof = pp.rho;\n";
    quiet(unknown, "E275");
    let steady = "\
steady_state_model;
foo = 1;
end;
foo = pp.rho;
";
    quiet(steady, "E275");
    let created = "\
parameters rho;
rho = 0.9 + undef;
undef = pp.rho;
";
    quiet(created, "E275");
    let external = "\
external_function(name=helper);
helper = pp.rho;
";
    quiet(external, "E275");
    agree(external, "E275");
}

#[test]
fn model_local_heads_still_report_e275() {
    let pound = "\
var y;
model;
#x = 1;
y = x;
end;
x = pp.rho;
";
    assert_eq!(message(&diags(pound), "E275"), NAMESPACE);
    agree(pound, "E275");
    let declared = "\
model_local_variable x;
x = pp.rho;
";
    assert_eq!(message(&diags(declared), "E275"), NAMESPACE);
    let same_line = "\
model_local_variable x;
x = 1; parameters rho; rho = pp.rho;
";
    // Slice 20 restores init_param's earlier target-type refusal. The RHS
    // namespace controls above still reach E275 before the target-type check.
    assert_eq!(message(&diags(same_line), "E378"), "x is not a parameter");
    let swallowed = "proof = 1; parameters rho; rho = pp.rho;\n";
    quiet(swallowed, "E275");
}

#[test]
fn a_generated_policy_discount_head_stays_an_expression() {
    let source = "\
var y pol;
model(linear);
y = 0;
end;
planner_objective y^2;
discretionary_policy(instruments=(pol));
optimal_policy_discount_factor = pp.rho;
";
    assert_eq!(message(&diags(source), "E275"), NAMESPACE);
    let model = parse(source);
    let name = model
        .intern
        .lookup("optimal_policy_discount_factor")
        .unwrap();
    assert!(model
        .param_assignments
        .iter()
        .any(|row| row.name == name && !row.native));
}

#[test]
fn reach_codes_are_quiet_on_a_native_rhs_and_still_fire_on_a_declared_head() {
    for native in ["proof = [1 2];\n", "proof = 'quote';\n", "proof = café;\n"] {
        quiet(native, "E001");
    }
    let bad_char = "parameters rho;\nrho = café;\n";
    assert!(has(&diags(bad_char), "E001"), "{:?}", diags(bad_char));
    let missing = "parameters alpha beta;\nalpha = 1\nbeta = 2;\n";
    assert!(has(&diags(missing), "E001"), "{:?}", diags(missing));
    for (native, declared, code) in [
        (
            "proof = log(0);\n",
            "parameters rho; rho = log(0);\n",
            "E276",
        ),
        ("proof = 1/0;\n", "parameters rho; rho = 1/0;\n", "E278"),
        (
            "external_function(name=myf); proof = myf;\n",
            "parameters rho; external_function(name=myf); rho = myf;\n",
            "E279",
        ),
    ] {
        quiet(native, code);
        assert!(
            has(&diags(declared), code),
            "{declared}: {:?}",
            diags(declared)
        );
    }
}

#[test]
fn a_pre_block_native_assignment_still_records_w012() {
    let source = "foo = 1;\nvar y;\nmodel;\ny = 1;\nend;\n";
    assert!(has(&diags(source), "W012"), "{:?}", diags(source));
    assert!(parse(source)
        .helper_assignments
        .iter()
        .any(|row| row.native));
}

#[test]
fn native_strings_trivia_and_continued_keywords_make_no_syntax_claim() {
    let sources = [
        "parameters rho; rho=1;\nproof = \"native\";\n",
        "parameters rho; rho=1; proof = \"native\";\n",
        "parameters rho; rho=1;\nproof = 1; /* keep\ngoing */ é\n",
        "parameters rho; rho=1; proof = 1; /* keep\ngoing */ é\n",
        "var y; model; y=1; end;\nproof = 1 ...\nparamters = 1;\n",
        "var y; varexo e; model; y=e; end;\nproof = 1 ...\nshoks = 1;\n",
    ];
    for source in sources {
        quiet(source, "E001");
        let control = format!("{source}parameters control;\ncontrol = pp.rho;\n");
        assert_eq!(message(&diags(&control), "E275"), NAMESPACE);
        agree(&control, "E275");
        if let Some(pp) = find_preprocessor(None) {
            let check =
                run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
            assert!(
                check.success,
                "{source}: {}{}",
                check.raw_stdout, check.raw_stderr
            );
        }
    }
    let declared = "parameters rho;\nrho = \"native\";\n";
    assert!(has(&diags(declared), "E001"));
    // A lone double quote has no matching NATIVE string rule.
    assert!(has(&diags("proof = \"\n"), "E001"));
}

#[test]
fn native_boundaries_follow_macro_output_in_execution_order() {
    let looped = "parameters rho;\n@#for i in 1:2\n@#if i == 2\nrho = pp.rho;\n@#endif\nproof = 1;\n@#endfor\n";
    let model = parse(looped);
    assert_eq!(model.helper_assignments.len(), 2);
    assert_eq!(model.param_assignments.len(), 1);
    assert_eq!(message(&analyze(&model), "E275"), NAMESPACE);
    agree(looped, "E275");

    let continued =
        "parameters rho;\nrho = 0.9;\n@#define tail = \"...\"\nproof = 1 @{tail}\nrho = pp.rho;\n";
    quiet(continued, "E275");
    agree(continued, "E275");
    assert_eq!(parse(continued).param_assignments.len(), 1);
    assert!(parse(continued).helper_assignments[0]
        .expression
        .contains("pp.rho"));

    if let Some(pp) = find_preprocessor(None) {
        let check = run_preprocessor(looped, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(!check.success);
        assert!(format!("{}{}", check.raw_stdout, check.raw_stderr).contains(NAMESPACE));
        let check = run_preprocessor(
            continued,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(check.success, "{}{}", check.raw_stdout, check.raw_stderr);
    }
}

#[test]
fn full_archive_is_quiet_for_native_assignment_claims_on_all_transports() {
    let source =
        include_str!("../.agents/skills/use-dynare/references/examples/NK_FNL23_rep_trans.mod");
    assert!(source
        .lines()
        .nth(310)
        .unwrap()
        .contains("oo_.steady_state(8)"));
    for code in ["E001", "E275"] {
        quiet(source, code);
        agree(source, code);
    }
    assert!(has(&diags(source), "W022"));
}

#[test]
fn dynare_accepts_the_native_heads_and_refuses_the_model_local_heads() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping honesty: Dynare preprocessor is absent");
        return;
    };
    for source in [
        "parameters rho; rho = 0.9;\nproof = pp.rho;\n",
        "steady_state_model; foo = 1; end;\nfoo = pp.rho;\n",
        "parameters rho; rho = 0.9 + undef;\nundef = pp.rho;\n",
        "external_function(name=helper); helper = pp.rho;\n",
        "parameters rho; rho = 0.9;\nproof = 1; rho = pp.rho;\n",
        "parameters rho; rho = 0.9;\nzz = 1; data(nobs=1);\n",
    ] {
        let check = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        assert!(
            check.success,
            "{source}: {}{}",
            check.raw_stdout, check.raw_stderr
        );
        quiet(source, "E275");
    }
    for source in [
        "parameters rho; rho = 0.9;\nproof = 1;\nrho = pp.rho;\n",
        "var y; model; #x = 1; y = x; end;\nx = pp.rho;\n",
        "model_local_variable x;\nx = pp.rho;\n",
    ] {
        let check = run_preprocessor(source, &pp, None, Duration::from_secs(30), JsonStage::Check);
        let text = format!("{}{}", check.raw_stdout, check.raw_stderr);
        assert!(text.contains(NAMESPACE), "{source}: {text}");
        assert_eq!(message(&diags(source), "E275"), NAMESPACE);
    }
}
