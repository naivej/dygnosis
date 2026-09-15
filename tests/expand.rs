use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::explain::known_codes;
use dygnosis::span::Span;
use dygnosis::{
    analyze, equations, expand_report, EquationOrigin, EquationRow, ExpandReport, OriginFrame,
    Workspace,
};

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expand_fixture(name: &str) -> PathBuf {
    fixtures_root().join("expand").join(name)
}

fn copilot_archive(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/model-archive")
        .join(name)
        .join(format!("{name}.mod"))
}

fn copilot_example(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".agents/skills/dynare-copilot/references/examples")
        .join(format!("{name}.mod"))
}

fn read_mod(path: &Path) -> String {
    fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("fixture missing at {}: {e}", path.display()))
        .replace("\r\n", "\n")
}

fn path_uri(path: &Path) -> String {
    path.to_str()
        .unwrap_or_else(|| panic!("non-utf8 path {}", path.display()))
        .to_string()
}

fn load_overlay(path: &Path) -> (Workspace, String, String) {
    let src = read_mod(path);
    let uri = path_uri(path);
    let mut ws = Workspace::new();
    ws.update_document(&uri, &src);
    (ws, uri, src)
}

fn uri_ends_with(uri: &str, suffix: &str) -> bool {
    Path::new(uri)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(suffix))
}

fn slice_of(src: &str, span: Span) -> &str {
    let start = span.start as usize;
    let end = span.end as usize;
    src.get(start..end)
        .unwrap_or_else(|| panic!("span {start}..{end} out of {}", src.len()))
}

fn relative_to_fixtures(origin_uri: &str) -> String {
    let fixtures_key = dygnosis::include_resolver::path_key(&fixtures_root());
    let origin = if cfg!(windows) {
        origin_uri.replace('/', "\\").to_lowercase()
    } else {
        origin_uri.to_string()
    };
    let prefix = if cfg!(windows) {
        fixtures_key.trim_end_matches('\\').to_string()
    } else {
        fixtures_key.trim_end_matches('/').to_string()
    };
    let rel = origin
        .strip_prefix(&prefix)
        .unwrap_or_else(|| panic!("origin {origin_uri} not under fixtures {prefix}"));
    let rel = rel.trim_start_matches(['\\', '/']);
    rel.replace('\\', "/")
}

fn dump_report(report: &ExpandReport) -> serde_json::Value {
    serde_json::json!({
        "effective_text": report.effective_text,
        "n_equations": report.n_equations,
        "origins": report.origins.iter().map(dump_origin).collect::<Vec<_>>(),
    })
}

fn dump_origin(origin: &EquationOrigin) -> serde_json::Value {
    serde_json::json!({
        "index": origin.index,
        "origin_span": {
            "start": origin.origin_span.start,
            "end": origin.origin_span.end,
        },
        "origin_uri": origin.origin_uri.as_deref().map(relative_to_fixtures),
        "origin_frames": origin.origin_frames.iter().map(dump_frame).collect::<Vec<_>>(),
    })
}

fn dump_frame(frame: &OriginFrame) -> serde_json::Value {
    serde_json::json!({
        "origin_span": {
            "start": frame.origin_span.start,
            "end": frame.origin_span.end,
        },
        "origin_uri": frame.origin_uri.as_deref().map(relative_to_fixtures),
        "kind": frame.kind,
    })
}

fn expected_dump(name: &str) -> serde_json::Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/expected/expand")
        .join(name);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("expected missing at {}: {e}", path.display()))
        .replace("\r\n", "\n");
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn assert_known_codes_only(src: &str, label: &str) {
    let known: Vec<&str> = known_codes();
    let codes: Vec<String> = analyze(&dygnosis::parse(src))
        .into_iter()
        .map(|d| d.code)
        .collect();
    for code in &codes {
        assert!(
            known.iter().any(|k| k == code),
            "{label}: new diagnostic family {code} (got {codes:?})"
        );
    }
}

#[test]
fn whole_eq_for_shared_body_span() {
    let path = expand_fixture("whole_eq_for.mod");
    let (mut ws, uri, src) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(report.n_equations, 3);
    assert_eq!(report.origins.len(), 3);
    assert_eq!(
        report.origins.iter().map(|o| o.index).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    let span = report.origins[0].origin_span;
    assert!(report.origins.iter().all(|o| o.origin_span == span));
    let slice = slice_of(&src, span);
    assert!(slice.contains("y = @{i}"), "origin slice {slice:?}");
    assert!(!slice.contains("@#define"), "origin slice {slice:?}");
    assert!(!slice.contains("@#for"), "origin slice {slice:?}");
    assert!(!slice.contains("@#endfor"), "origin slice {slice:?}");
    for origin in &report.origins {
        assert!(origin.origin_frames.is_empty());
        let uri = origin.origin_uri.as_deref().expect("workspace origin_uri");
        assert!(uri_ends_with(uri, "whole_eq_for.mod"), "origin_uri {uri}");
    }
    assert!(report.effective_text.contains("y = 1"));
    assert!(report.effective_text.contains("y = 2"));
    assert!(report.effective_text.contains("y = 3"));
}

#[test]
fn nested_for_origin_frames() {
    let path = expand_fixture("nested_for.mod");
    let (mut ws, uri, src) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(report.n_equations, 4);
    assert_eq!(report.origins.len(), 4);
    let span = report.origins[0].origin_span;
    assert!(report.origins.iter().all(|o| o.origin_span == span));
    for origin in &report.origins {
        assert_eq!(origin.origin_frames.len(), 2);
        assert!(origin.origin_frames.iter().all(|f| f.kind == "for"));
        assert_eq!(origin.origin_span, origin.origin_frames[1].origin_span);
        let inner = origin.origin_frames.last().unwrap();
        let inner_text = slice_of(&src, inner.origin_span);
        assert!(inner_text.contains("x = @{i}"), "inner body {inner_text:?}");
        assert!(!inner_text.contains("@#for"), "inner body {inner_text:?}");
        let outer = &origin.origin_frames[0];
        let outer_text = slice_of(&src, outer.origin_span);
        assert!(outer_text.contains("@#for j"), "outer body {outer_text:?}");
        let uri = origin.origin_uri.as_deref().expect("workspace origin_uri");
        assert!(uri_ends_with(uri, "nested_for.mod"), "origin_uri {uri}");
    }
}

#[test]
fn include_eq_row_origins() {
    let path = expand_fixture("include_eq.mod");
    let inc_path = expand_fixture("include_eq_body.inc");
    let src = read_mod(&path);
    let inc_src = read_mod(&inc_path);
    let uri = path_uri(&path);
    let inc_uri = path_uri(&inc_path);
    let mut ws = Workspace::new();
    ws.update_document(&uri, &src);
    ws.update_document(&inc_uri, &inc_src);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(report.n_equations, 2);
    let row0 = &report.origins[0];
    let uri0 = row0.origin_uri.as_deref().expect("row0 uri");
    assert!(uri_ends_with(uri0, "include_eq.mod"), "row0 uri {uri0}");
    assert_eq!(slice_of(&src, row0.origin_span), "y = 1");
    assert!(row0.origin_frames.is_empty());
    let row1 = &report.origins[1];
    let uri1 = row1.origin_uri.as_deref().expect("row1 uri");
    assert!(
        uri_ends_with(uri1, "include_eq_body.inc"),
        "row1 uri {uri1}"
    );
    assert!(
        slice_of(&inc_src, row1.origin_span).contains("z = 0"),
        "inc slice {:?}",
        slice_of(&inc_src, row1.origin_span)
    );
    assert!(row1.origin_frames.is_empty());
    assert!(report.effective_text.contains("z = 0"));
}

#[test]
fn free_expand_report_whole_eq_for_uris_none() {
    let src = read_mod(&expand_fixture("whole_eq_for.mod"));
    let path = expand_fixture("whole_eq_for.mod");
    let (mut ws, uri, _) = load_overlay(&path);
    let ws_report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()))
        .clone();
    let free = expand_report(&src);
    assert_eq!(free.n_equations, ws_report.n_equations);
    assert_eq!(free.n_equations, 3);
    let spans: Vec<_> = free.origins.iter().map(|o| o.origin_span).collect();
    let ws_spans: Vec<_> = ws_report.origins.iter().map(|o| o.origin_span).collect();
    assert_eq!(spans, ws_spans);
    assert!(spans.windows(2).all(|w| w[0] == w[1]));
    assert!(free.origins.iter().all(|o| o.origin_uri.is_none()));
    assert_eq!(free.effective_text, ws_report.effective_text);
}

#[test]
fn free_expand_report_include_eq_does_not_splice() {
    let src = read_mod(&expand_fixture("include_eq.mod"));
    let free = expand_report(&src);
    assert!(
        !free.effective_text.contains("z = 0"),
        "free expand spliced include: {}",
        free.effective_text
    );
    assert!(
        free.effective_text.contains("y = 1"),
        "free expand missing y = 1: {}",
        free.effective_text
    );
    assert_eq!(free.n_equations, 1);
}

#[test]
fn us_re09_rep_phillips_source_statement() {
    let path = copilot_example("US_RE09_rep");
    let (mut ws, uri, src) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(report.n_equations, 19);
    assert_eq!(report.origins.len(), 19);
    for lag in 1..=16 {
        let needle = format!("EXPECTATION(-{lag})");
        assert!(report.effective_text.contains(&needle), "missing {needle}");
    }
    let row = &report.origins[2];
    assert!(row.origin_frames.is_empty());
    let origin_uri = row.origin_uri.as_deref().expect("origin_uri");
    assert!(
        uri_ends_with(origin_uri, "US_RE09_rep.mod"),
        "origin_uri {origin_uri}"
    );
    let slice = slice_of(&src, row.origin_span);
    assert!(slice.contains("p = lambda"), "phillips origin {slice:?}");
    assert!(
        slice.contains("@#for lag in lags"),
        "phillips origin {slice:?}"
    );
    assert!(slice.contains("@#endfor"), "phillips origin {slice:?}");
    assert!(!slice.contains("// IS Curve"), "phillips origin {slice:?}");
}

#[test]
fn zlb_qe_inactive_qe_absent() {
    let path = copilot_archive("zlb_qe");
    let (mut ws, uri, src) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(report.n_equations, 16);
    assert!(report.effective_text.contains("qe = 0"));
    assert!(!report.effective_text.contains("qe = rho_qe"));
    assert_eq!(report.origins.len(), 16);
    let f17 = &report.origins[14];
    let origin_uri = f17.origin_uri.as_deref().expect("f17 uri");
    assert!(
        uri_ends_with(origin_uri, "zlb_qe.mod"),
        "f17 uri {origin_uri}"
    );
    assert!(f17.origin_frames.is_empty());
    let slice = slice_of(&src, f17.origin_span);
    assert!(slice.contains("qe = 0"), "f17 origin {slice:?}");
    assert!(!slice.contains("rho_qe"), "f17 origin {slice:?}");
}

#[test]
fn swff_splices_params_equation_origin_stays_mod() {
    let path = copilot_archive("swff");
    let (mut ws, uri, _) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert!(
        report.effective_text.contains("alppha"),
        "effective_text missing alppha"
    );
    assert!(
        report.effective_text.contains("0.178678"),
        "effective_text missing 0.178678"
    );
    assert!(report.n_equations > 0);
    assert_eq!(report.origins.len(), report.n_equations);
    for origin in &report.origins {
        let origin_uri = origin.origin_uri.as_deref().expect("origin_uri");
        assert!(
            uri_ends_with(origin_uri, "swff.mod"),
            "counted origin_uri {origin_uri}"
        );
        assert!(
            !uri_ends_with(origin_uri, "swff_params.inc"),
            "counted origin_uri {origin_uri}"
        );
    }
}

#[test]
fn include_overlay_invalidates_parent_expand() {
    let path = expand_fixture("include_eq.mod");
    let inc_path = expand_fixture("include_eq_body.inc");
    let src = read_mod(&path);
    let inc_src = read_mod(&inc_path);
    let uri = path_uri(&path);
    let inc_uri = path_uri(&inc_path);
    let mut ws = Workspace::new();
    ws.update_document(&uri, &src);
    ws.update_document(&inc_uri, &inc_src);
    let first = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()))
        .effective_text
        .clone();
    assert!(
        first.contains("z = 0"),
        "first expand missing z = 0: {first}"
    );
    ws.update_document(&inc_uri, "z = 9;\n");
    let second = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report after overlay {}", path.display()));
    assert!(
        second.effective_text.contains("z = 9"),
        "after overlay: {}",
        second.effective_text
    );
    assert!(
        !second.effective_text.contains("z = 0"),
        "stale after overlay: {}",
        second.effective_text
    );
}

#[test]
fn whole_eq_for_dump() {
    let path = expand_fixture("whole_eq_for.mod");
    let (mut ws, uri, _) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(dump_report(report), expected_dump("whole_eq_for.json"));
}

#[test]
fn nested_for_dump() {
    let path = expand_fixture("nested_for.mod");
    let (mut ws, uri, _) = load_overlay(&path);
    let report = ws
        .expand_report(&uri)
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(dump_report(report), expected_dump("nested_for.json"));
}

#[test]
fn include_eq_dump() {
    let path = expand_fixture("include_eq.mod");
    let inc_path = expand_fixture("include_eq_body.inc");
    let mut ws = Workspace::new();
    ws.update_document(&path_uri(&path), read_mod(&path));
    ws.update_document(&path_uri(&inc_path), read_mod(&inc_path));
    let report = ws
        .expand_report(&path_uri(&path))
        .unwrap_or_else(|| panic!("expand_report {}", path.display()));
    assert_eq!(dump_report(report), expected_dump("include_eq.json"));
}

#[test]
fn us_re09_equation_keys_unchanged() {
    let path = copilot_example("US_RE09_rep");
    let (mut ws, uri, _) = load_overlay(&path);
    let model = ws
        .get_effective_model(&uri)
        .unwrap_or_else(|| panic!("get_effective_model {}", path.display()));
    let rows = equations(model);
    assert_eq!(rows.len(), 19);
    let indexes: Vec<_> = rows.iter().map(|r| r.index).collect();
    assert_eq!(indexes, (0..19).collect::<Vec<_>>());
    assert!(
        rows[2].text.contains("EXPECTATION(-16)"),
        "phillips text {}",
        rows[2].text
    );
    for row in &rows {
        let EquationRow {
            index: _,
            name: _,
            text: _,
            lhs: _,
            rhs: _,
            span: _,
            static_tag: _,
            dynamic_tag: _,
            idents: _,
        } = row;
    }
}

#[test]
fn original_expand_fixtures_add_no_new_code_family() {
    for name in ["whole_eq_for.mod", "nested_for.mod", "include_eq.mod"] {
        let src = read_mod(&expand_fixture(name));
        assert_known_codes_only(&src, name);
    }
}
