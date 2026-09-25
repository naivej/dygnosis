//! Added-Warning comment suppression. One policy for check, LSP, MCP, and fixes.

use std::fs;
use std::path::Path;

use dygnosis::server::diagnostics_for;
use dygnosis::{auto_fix, check_file, dynare_diagnose, format_check_lines};
use tower_lsp::lsp_types::NumberOrString;

fn gap() -> &'static str {
    "\
parameters a;
var y z;
model;
y = a;
end;
"
}

fn has(codes: &[String], code: &str) -> bool {
    codes.iter().any(|c| c == code)
}

fn check_codes(text: &str) -> Vec<String> {
    check_file(text, "C:/tmp/suppress.mod")
        .into_iter()
        .map(|d| d.code)
        .collect()
}

fn lsp_codes(text: &str) -> Vec<String> {
    diagnostics_for("file:///C:/tmp/suppress.mod", text)
        .into_iter()
        .filter_map(|d| match d.code {
            Some(NumberOrString::String(code)) => Some(code),
            _ => None,
        })
        .collect()
}

fn mcp_codes(text: &str) -> Vec<String> {
    dynare_diagnose(text, None, None)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

fn assert_same_quiet(text: &str, quiet: &[&str], keep: &[&str]) {
    for codes in [check_codes(text), lsp_codes(text), mcp_codes(text)] {
        for code in quiet {
            assert!(
                !has(&codes, code),
                "{code} still present in {codes:?}\n{text}"
            );
        }
        for code in keep {
            assert!(has(&codes, code), "{code} missing from {codes:?}\n{text}");
        }
    }
}

#[test]
fn line_next_line_file_and_legacy_forms() {
    let bare = check_codes(gap());
    assert!(has(&bare, "W010"), "{bare:?}");
    assert!(has(&bare, "W013"), "{bare:?}");

    let same_line = gap().replace("parameters a;", "parameters a; // dygnosis:disable W010");
    assert_same_quiet(&same_line, &["W010"], &["W013"]);

    let next = format!("// dygnosis:disable-next-line W010\n{}", gap());
    assert_same_quiet(&next, &["W010"], &["W013"]);

    let two_later = format!("// dygnosis:disable-next-line W010\n\n{}", gap());
    assert_same_quiet(&two_later, &[], &["W010", "W013"]);

    let file = format!("// dygnosis:disable-file W010, W013\n{}", gap());
    assert_same_quiet(&file, &["W010", "W013"], &[]);

    let legacy = format!(
        "// vsd:disable-file W013\n{}",
        gap().replace("parameters a;", "parameters a; // vsd:disable W010")
    );
    assert_same_quiet(&legacy, &["W010", "W013"], &[]);

    let legacy_line = gap().replace("parameters a;", "parameters a; // vsd:disable W010");
    assert_same_quiet(&legacy_line, &["W010"], &["W013"]);
}

#[test]
fn malformed_and_ineligible_directives_do_nothing() {
    let shapes = [
        format!("// dygnosis:disable\n{}", gap()),
        format!("// dygnosis:disable W010,\n{}", gap()),
        format!("// dygnosis:disable W010 extra\n{}", gap()),
        format!("// vsd:disable-next-line W010\n{}", gap()),
        format!("// dygnosis:enable W010\n{}", gap()),
        format!("// dygnosis:disable NOPE\n{}", gap()),
        format!("// dygnosis:disable-file E001, W031, I050\n{}", gap()),
    ];
    for text in shapes {
        assert_same_quiet(&text, &[], &["W010", "W013"]);
    }
    let square = "\
// dygnosis:disable-file I050
parameters a;
a = 1;
var y;
model;
y = a;
end;
";
    assert_same_quiet(square, &[], &["I050"]);
}

#[test]
fn error_and_shared_warning_stay() {
    let text = "\
// dygnosis:disable-file E001, W013
parameters betta;
betta = 0.99
var y;
model;
y = betta;
end;
";
    assert_same_quiet(text, &[], &["E001"]);

    let shared = "\
// dygnosis:disable-file W031, W013
var y;
var y;
model;
y = 0;
end;
";
    assert_same_quiet(shared, &["W013"], &["W031"]);
}

#[test]
fn strings_native_verbatim_and_inactive_macros_are_not_directives() {
    let string = "\
parameters a;
var y z;
model;
y = a;
end;
not_a_command('// dygnosis:disable-file W010, W013');
";
    assert_same_quiet(string, &[], &["W010", "W013"]);

    let native = "\
parameters a;
var y z;
model;
y = a;
end;
not_a_command(1 + // dygnosis:disable-file W010, W013
0);
";
    assert_same_quiet(native, &[], &["W010", "W013"]);

    let verbatim = format!(
        "verbatim;\n// dygnosis:disable-file W010, W013\nend;\n{}",
        gap()
    );
    assert_same_quiet(&verbatim, &[], &["W010", "W013"]);

    let inactive = format!(
        "@#if 0\n// dygnosis:disable-file W010, W013\n@#endif\n{}",
        gap()
    );
    assert_same_quiet(&inactive, &[], &["W010", "W013"]);

    let ifdef = format!(
        "@#ifdef MISSING\n// dygnosis:disable-file W010, W013\n@#endif\n{}",
        gap()
    );
    assert_same_quiet(&ifdef, &[], &["W010", "W013"]);

    let live_sum = format!(
        "@#if 1+1\n// dygnosis:disable-file W010, W013\n@#endif\n{}",
        gap()
    );
    assert_same_quiet(&live_sum, &["W010", "W013"], &[]);

    let elseif_live = format!(
        "@#if 0\n@#elseif 1\n// dygnosis:disable-file W010, W013\n@#endif\n{}",
        gap()
    );
    assert_same_quiet(&elseif_live, &["W010", "W013"], &[]);

    let block = format!("/* // dygnosis:disable-file W010, W013 */\n{}", gap());
    assert_same_quiet(&block, &[], &["W010", "W013"]);
}

#[test]
fn loop_instances_follow_the_source_line() {
    let quiet = "\
@#define is = 1:2
var y;
model;
@#for i in is
y = 0; // dygnosis:disable W054
@#endfor
end;
";
    assert_same_quiet(quiet, &["W054"], &["W013"]);

    let loud = "\
@#define is = 1:2
var y;
model;
@#for i in is
y = 0;
@#endfor
end;
";
    assert_same_quiet(loud, &[], &["W054", "W013"]);
}

#[test]
fn include_file_directive_stays_in_that_file() {
    let dir = std::env::temp_dir().join(format!("dyg-suppress-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let parent = dir.join("parent.mod");
    let child = dir.join("child.mod");
    let other = dir.join("other.mod");

    fs::write(&child, "model;\ny = 0;\nend;\n").unwrap();
    fs::write(&other, "// dygnosis:disable-file W013\nparameters p;\n").unwrap();
    fs::write(
        &parent,
        "@#include \"child.mod\"\n@#include \"other.mod\"\nvar y;\nvar y;\n",
    )
    .unwrap();
    let parent_text = fs::read_to_string(&parent).unwrap();
    let sibling = check_file(&parent_text, parent.to_str().unwrap());
    let codes: Vec<_> = sibling.iter().map(|d| d.code.clone()).collect();
    assert!(
        has(&codes, "W013"),
        "sibling directive suppressed W013: {codes:?}"
    );
    assert!(has(&codes, "W031"), "W031 missing: {codes:?}");

    fs::write(
        &parent,
        "// dygnosis:disable-file W013\n@#include \"child.mod\"\n@#include \"other.mod\"\nvar y;\nvar y;\n",
    )
    .unwrap();
    let with_info = fs::read_to_string(&parent).unwrap();
    let info_codes: Vec<_> = check_file(&with_info, parent.to_str().unwrap())
        .into_iter()
        .map(|d| d.code)
        .collect();
    assert!(
        has(&info_codes, "W013"),
        "parent file directive suppressed the included model: {info_codes:?}"
    );

    fs::write(
        &child,
        "// dygnosis:disable-file W013, W031\nmodel;\ny = 0;\nend;\n",
    )
    .unwrap();
    let parent_text = fs::read_to_string(&parent).unwrap();
    let own = check_file(&parent_text, parent.to_str().unwrap());
    let codes: Vec<_> = own.iter().map(|d| d.code.clone()).collect();
    assert!(
        !has(&codes, "W013"),
        "child file directive missed W013: {codes:?}"
    );
    assert!(
        has(&codes, "W031"),
        "child file directive crossed to parent W031: {codes:?}"
    );

    let mcp = dynare_diagnose(
        &parent_text,
        Some(parent.to_str().unwrap()),
        Some(&files_map(&dir, &["parent.mod", "child.mod", "other.mod"])),
    );
    let mcp_codes: Vec<_> = mcp.iter().map(|d| d.code.clone()).collect();
    assert!(!has(&mcp_codes, "W013"), "{mcp_codes:?}");
    assert!(has(&mcp_codes, "W031"), "{mcp_codes:?}");

    let cli = format_check_lines(parent.to_str().unwrap(), &own, &parent_text);
    assert!(!cli.contains("[W013]"), "{cli}");
    assert!(cli.contains("[W031]"), "{cli}");
    let warnings = cli.lines().last().unwrap();
    assert!(warnings.contains("warning"), "{warnings}");

    let _ = fs::remove_dir_all(&dir);
}

fn files_map(dir: &Path, names: &[&str]) -> std::collections::HashMap<String, String> {
    names
        .iter()
        .map(|name| {
            let path = dir.join(name);
            (
                path.to_str().unwrap().to_string(),
                fs::read_to_string(path).unwrap(),
            )
        })
        .collect()
}

#[test]
fn fixes_still_apply_an_error_and_skip_a_suppressed_warning() {
    let broken = "\
// dygnosis:disable-file E001, W013
parameters betta;
betta = 0.99
var y;
model;
y = betta;
end;
";
    let fixed = auto_fix(broken);
    assert!(fixed.contains("betta = 0.99;"), "{fixed}");
    assert!(!check_codes(&fixed).iter().any(|c| c == "E001"), "{fixed}");
}
