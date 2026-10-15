use std::path::{Path, PathBuf};

use dygnosis::{format_range, format_text};

const ARCHIVES: &[&str] = &[
    "trend_rbc_gov_inv",
    "sims_wu_2019",
    "govt_rbc_irf_matching",
    "lk2024",
    "zlb_qe",
    "swff",
    "bj2021_demography",
    "bgg_financial",
];

const PAIRS: &[&str] = &[
    "tight_ops",
    "lead_lag",
    "macro_line",
    "eqeq",
    "occbin_block",
];

fn copilot_mod(archive_dir: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/use-dynare/references/model-archive")
        .join(archive_dir)
        .join(format!("{archive_dir}.mod"))
}

fn format_fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/format")
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn read_mod(archive_dir: &str) -> String {
    read_text(&copilot_mod(archive_dir))
}

fn trend() -> String {
    read_mod("trend_rbc_gov_inv")
}

fn assert_pair(name: &str) {
    let input = read_text(&format_fixtures().join(format!("{name}.mod")));
    let want = read_text(&format_fixtures().join(format!("{name}.formatted.mod")));
    let rust = format_text(&input, "\t").unwrap_or_else(|| panic!("{name} declined"));
    assert_eq!(rust, want, "formatted text for {name}");
    assert_ne!(rust, input, "{name} must change the input");
    assert_eq!(
        format_text(&want, "\t"),
        None,
        "{name} second pass must decline"
    );
}

fn line_of(src: &str, needle: &str) -> u32 {
    let pos = src
        .find(needle)
        .unwrap_or_else(|| panic!("missing {needle}"));
    src[..pos].bytes().filter(|&b| b == b'\n').count() as u32
}

fn slice_lines(text: &str, start: u32, end: u32) -> String {
    let raw: Vec<&str> = text.split('\n').collect();
    raw[start as usize..=end as usize].join("\n")
}

#[test]
fn format_fixture_pairs() {
    for name in PAIRS {
        assert_pair(name);
    }
}

#[test]
fn empty_and_ws_only_decline() {
    assert_eq!(format_text("", "\t"), None);
    assert_eq!(format_text("   \n", "\t"), None);
    assert_eq!(format_text(" \n\t\n", "\t"), None);
}

#[test]
fn trailing_comment_space_stripped() {
    let src = "// comment  \nvar y;\nmodel;\ny=1;\nend;\n";
    let out = format_text(src, "\t").expect("should format");
    assert!(
        !out.lines().any(|l| l.ends_with(' ')),
        "trailing spaces remain: {out:?}"
    );
    assert!(out.contains("// comment"));
    assert_eq!(format_text(&out, "\t"), None);
}

#[test]
fn extra_blank_lines_collapsed() {
    let src = "var y;\n\n\nmodel;\ny=1;\nend;\n";
    let out = format_text(src, "\t").expect("should format");
    assert!(!out.contains("\n\n\n"), "triple blank remains: {out:?}");
    assert_eq!(format_text(&out, "\t"), None);
}

#[test]
fn archives_format_without_exploding() {
    for name in ARCHIVES {
        let src = read_mod(name);
        match format_text(&src, "\t") {
            None => {}
            Some(out) => {
                assert_ne!(out, src, "{name} Some(out) must differ from input");
                assert_eq!(
                    format_text(&out, "\t"),
                    None,
                    "{name} second pass must decline"
                );
            }
        }
    }
}

#[test]
fn already_formatted_declines() {
    let src = trend();
    if let Some(formatted) = format_text(&src, "\t") {
        assert_eq!(format_text(&formatted, "\t"), None);
    } else {
        assert_eq!(format_text(&src, "\t"), None);
    }
}

#[test]
fn preserves_crlf() {
    let lf = "var y;\nmodel;\ny=1;\nend;\n";
    let crlf = lf.replace('\n', "\r\n");
    let out = format_text(&crlf, "\t").expect("CRLF input should format");
    assert!(out.contains("\r\n"), "must keep CRLF");
    assert!(!out.replace("\r\n", "").contains('\n'), "no bare LF");
    assert_ne!(out, crlf);
    assert!(out.contains("y = 1;"), "spaces around =, got {out:?}");
}

// Guards command arguments being split by operator spacing, including a native
// tail after a Dynare statement and arguments continued onto another line.
#[test]
fn native_command_arguments_survive_document_and_range_formatting() {
    for native in [
        "disp hello-world",
        "save results -ascii",
        "parameters a; a=1; disp hello-world",
        "x=1, disp hello-world",
        "if(true), disp hello-world, end",
        "foo(); model;\ndisp hello-world",
        "@#if 0\nmodel;\n@#endif\ndisp hello-world",
        "@#if 0\ndisp hello-world\n@#endif",
        "@#if 0\nverbatim;\ndisp hello-world\nend;\n@#endif",
        "@#define BAD 1\ndisp hello-world",
        "verbatim;\ndisp hello-world\nend;",
        "disp hello-world ...\n1+2",
        "disp hello-world ...\nend;",
    ] {
        let source = format!("var y;\nmodel;\ny=1;\nend;\n{native}\n");
        let formatted = format_text(&source, "\t").expect("model equation formats");
        assert!(formatted.contains("\ty = 1;"), "{formatted}");
        assert!(
            formatted.contains(native),
            "native arguments changed: {formatted}"
        );
        assert_eq!(format_text(&formatted, "\t"), None);
        let end = source.lines().count() as u32 - 1;
        let (_, _, replacement) = format_range(&source, 2, end, "\t")
            .expect("range containing the model equation formats");
        assert!(
            replacement.contains(native),
            "native range changed: {replacement}"
        );
        assert_eq!(format_range(&source, 4, end, "\t"), None);
    }
}

#[test]
fn format_range_assignments() {
    let src = trend();
    let start = line_of(&src, "betta   = 0.99;");
    let end = line_of(&src, "deltag  = 0.05;");
    let rust = format_range(&src, start, end, "\t").expect("param range");
    assert_eq!((rust.0, rust.1), (start, end));
    assert!(rust.2.contains("betta = 0.99;"));
    assert!(rust.2.contains("deltag = 0.05;"));
    assert!(!rust.2.contains("betta   ="));
    let original = slice_lines(&src, start, end);
    assert_ne!(rust.2, original);

    let start = line_of(&src, "y = exp(z) * kg(-1)^alphag");
    let end = line_of(&src, "rk = alppha * y / k(-1);");
    let rust = format_range(&src, start, end, "\t").expect("model range");
    assert_eq!((rust.0, rust.1), (start, end));
    assert!(rust
        .2
        .contains("y = exp(z) * kg(-1) ^ alphag * k(-1) ^ alppha * n ^ (1 - alppha - alphag);"));
    assert!(rust.2.contains("rk = alppha * y / k(-1);"));
    let original = slice_lines(&src, start, end);
    assert_ne!(rust.2, original);
}

// Guards omitted block heads and multiline headers using parser boundaries,
// including the range formatter's initial depth and the following root command.
#[test]
fn retained_block_boundaries_control_whole_and_range_indentation() {
    let source = "var y;\nvarexo e;\nparameters p;\np=0.5;\nmodel(\nlinear\n);\ny=p*y(-1)+e;\nend;\nconditional_forecast_paths;\nvar y;\nperiods 1;\nvalues 2;\nend;\nmshocks;\nvar e;\nperiods 1;\nvalues 0.5;\nend;\nsteady;\n";
    let formatted = format_text(source, "\t").expect("block bodies need indentation");
    for body in ["y = p * y(-1) + e;", "var y;", "periods 1;", "values 0.5;"] {
        assert!(
            formatted.contains(&format!("\n\t{body}\n")),
            "{body}: {formatted}"
        );
    }
    assert!(formatted.ends_with("end;\nsteady;\n"));
    assert_eq!(format_text(&formatted, "\t"), None);
    let start = line_of(source, "var y;\nperiods");
    let end = line_of(source, "values 2;");
    let (_, _, replacement) = format_range(source, start, end, "\t").expect("range body");
    assert_eq!(replacement, "\tvar y;\n\tperiods 1;\n\tvalues 2;");
    let before = dygnosis::parser::parse(source);
    let after = dygnosis::parser::parse(&formatted);
    let structure = |model: &dygnosis::model::Model| {
        model
            .statements
            .iter()
            .map(|row| (row.name.clone(), row.kind, row.complete))
            .collect::<Vec<_>>()
    };
    assert_eq!(structure(&before), structure(&after));
}

// Guards a closer discarding an opener on the same physical line. Whole and
// range formatting must agree on the following body's depth and root command.
#[test]
fn mixed_close_open_lines_keep_following_block_depth() {
    for opener in ["initval", "model"] {
        for indent in ["\t", "  "] {
            let source = format!("var y;\nmodel;\ny=1;\n   end; {opener};\ny=2;\nend;\nsteady;\n");
            let formatted = format_text(&source, indent).expect("block bodies need indentation");
            assert!(
                formatted.contains(&format!(
                    "\nend; {opener};\n{indent}y = 2;\nend;\nsteady;\n"
                )),
                "{formatted}"
            );
            assert_eq!(format_text(&formatted, indent), None);
            let (_, _, block_range) =
                format_range(&source, 3, 5, indent).expect("mixed transition and body range");
            assert_eq!(block_range, format!("end; {opener};\n{indent}y = 2;\nend;"));
            let (_, _, body_range) =
                format_range(&source, 4, 4, indent).expect("following body range");
            assert_eq!(body_range, format!("{indent}y = 2;"));
            assert_eq!(format_range(&formatted, 3, 6, indent), None);
            assert_eq!(format_range(&source, 6, 6, indent), None);
            let structure = |text: &str| {
                dygnosis::parse(text)
                    .statements
                    .iter()
                    .map(|row| (row.name.clone(), row.kind, row.complete))
                    .collect::<Vec<_>>()
            };
            assert_eq!(structure(&source), structure(&formatted));
        }
    }
}
