use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use dygnosis::server::new_service;
use dygnosis::{
    dynare_related_files, dynare_workspace_diagnose, CompanionKind, CompanionRecord, Workspace,
};
use tower_lsp::{lsp_types::*, LanguageServer};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-qualified-companions-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn file(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, source).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn records(workspace: &mut Workspace, root: &Path) -> Vec<CompanionRecord> {
    workspace
        .companion_records(root.to_str().unwrap())
        .unwrap()
        .to_vec()
}

fn named<'a>(rows: &'a [CompanionRecord], name: &str) -> &'a CompanionRecord {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("missing {name}: {rows:?}"))
}

fn assert_path(row: &CompanionRecord, path: &Path) {
    assert_eq!(row.kind, CompanionKind::HelperM);
    assert_eq!(
        fs::canonicalize(row.path.as_ref().unwrap()).unwrap(),
        fs::canonicalize(path).unwrap()
    );
}

#[test]
fn external_options_keep_complete_names_ranges_and_package_paths() {
    let fixture = Fixture::new();
    let source = "external_function(name=pkg.sub.foo,nargs=1,first_deriv_provided=pkg . deriv . jac,second_deriv_provided='pkg.deriv.hess'); var y; model; y=pkg.sub.foo(y(-1)); end;";
    let root = fixture.file("model.mod", source);
    let foo = fixture.file("+pkg/+sub/foo.m", "% helper");
    let jac = fixture.file("+pkg/+deriv/jac.m", "% jacobian");
    let hess = fixture.file("+pkg/+deriv/hess.m", "% hessian");
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    for (name, written, path) in [
        ("pkg.sub.foo", "pkg.sub.foo", foo),
        ("pkg.deriv.jac", "pkg . deriv . jac", jac),
        ("pkg.deriv.hess", "'pkg.deriv.hess'", hess),
    ] {
        let row = named(&rows, name);
        assert_path(row, &path);
        assert_eq!(
            &source[row.named_in.start as usize..row.named_in.end as usize],
            written
        );
    }
    assert_eq!(rows.len(), 3, "{rows:?}");
}

#[test]
fn qualified_misses_do_not_resolve_prefix_or_leaf_files_and_flags_add_no_file() {
    let fixture = Fixture::new();
    let source = "external_function(name=pkg.foo,first_deriv_provided,second_deriv_provided); external_function(name=other.foo,nargs=1); var y; model; y=0; end; steady_state_model; y=pkg.foo(1); end;";
    let root = fixture.file("model.mod", source);
    fixture.file("pkg.m", "% wrong prefix");
    fixture.file("foo.m", "% wrong leaf");
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    assert_eq!(rows.len(), 2, "{rows:?}");
    for name in ["pkg.foo", "other.foo"] {
        assert!(named(&rows, name).path.is_none());
    }
}

#[test]
fn qualified_calls_resolve_packages_without_leaf_aliases_and_deduplicate_declarations() {
    let fixture = Fixture::new();
    let source = "external_function(name=pkg.foo,nargs=1); var y; model; y=0; end; steady_state_model; y=pkg.foo(other.sub.foo(1)); end; native.sub.foo(1);";
    let root = fixture.file("model.mod", source);
    let paths = [
        fixture.file("+pkg/foo.m", "% first"),
        fixture.file("+other/+sub/foo.m", "% second"),
        fixture.file("+native/+sub/foo.m", "% native"),
    ];
    fixture.file("foo.m", "% unrelated leaf");
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    assert_eq!(rows.len(), 3, "{rows:?}");
    for (name, path) in ["pkg.foo", "other.sub.foo", "native.sub.foo"]
        .iter()
        .zip(paths)
    {
        assert_path(named(&rows, name), &path);
    }
    assert_eq!(
        named(&rows, "pkg.foo").named_in.start as usize,
        source.find("pkg.foo").unwrap()
    );
}

#[test]
fn explicit_quoted_paths_and_unqualified_helpers_keep_their_file_semantics() {
    let fixture = Fixture::new();
    let source = "external_function(name='helpers/foo.m',first_deriv_provided='foo.m'); external_function(name=pkg.m); external_function(name=plain);";
    let root = fixture.file("model.mod", source);
    let paths = [
        fixture.file("helpers/foo.m", "% path"),
        fixture.file("foo.m", "% literal"),
        fixture.file("+pkg/m.m", "% function m"),
        fixture.file("plain.m", "% ordinary"),
    ];
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    assert_eq!(rows.len(), 4, "{rows:?}");
    for (name, path) in ["helpers/foo.m", "foo.m", "pkg.m", "plain"]
        .iter()
        .zip(paths)
    {
        assert_path(named(&rows, name), &path);
    }
}

#[test]
fn scoped_search_paths_resolve_packages_and_isolate_roots() {
    let fixture = Fixture::new();
    let root_a = fixture.file("a/model.mod", "external_function(name=pkg.foo);");
    let root_b = fixture.file("b/model.mod", "external_function(name=pkg.foo);");
    let path_a = fixture.file("search-a/+pkg/foo.m", "% a");
    let path_b = fixture.file("search-b/+pkg/foo.m", "% b");
    let mut workspace = Workspace::new();
    for (root, search) in [(&root_a, "search-a"), (&root_b, "search-b")] {
        workspace.load_from_disk(root).unwrap();
        workspace.set_root_search_paths(root.to_str().unwrap(), vec![fixture.0.join(search)]);
    }
    assert_path(named(&records(&mut workspace, &root_a), "pkg.foo"), &path_a);
    assert_path(named(&records(&mut workspace, &root_b), "pkg.foo"), &path_b);
}

#[test]
fn package_file_create_change_delete_and_overlay_edits_change_root_revision() {
    let fixture = Fixture::new();
    let root = fixture.file("model.mod", "external_function(name=pkg.sub.foo);");
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let uri = root.to_str().unwrap();
    let before = workspace.input_revision(uri).unwrap();
    assert!(named(&records(&mut workspace, &root), "pkg.sub.foo")
        .path
        .is_none());
    let helper = fixture.file("+pkg/+sub/foo.m", "% created");
    let created = workspace.input_revision(uri).unwrap();
    assert_ne!(before, created);
    assert_path(
        named(&records(&mut workspace, &root), "pkg.sub.foo"),
        &helper,
    );
    fs::write(&helper, "% changed").unwrap();
    let changed = workspace.input_revision(uri).unwrap();
    assert_ne!(created, changed);
    workspace.update_document(helper.to_str().unwrap(), "% overlay");
    let overlay = workspace.input_revision(uri).unwrap();
    assert_ne!(changed, overlay);
    fs::write(&helper, "% hidden disk change").unwrap();
    assert_eq!(overlay, workspace.input_revision(uri).unwrap());
    workspace.remove_document(helper.to_str().unwrap());
    let restored = workspace.input_revision(uri).unwrap();
    assert_ne!(overlay, restored);
    fs::remove_file(&helper).unwrap();
    assert_ne!(restored, workspace.input_revision(uri).unwrap());
    assert!(named(&records(&mut workspace, &root), "pkg.sub.foo")
        .path
        .is_none());
}

#[test]
fn macro_active_qualified_options_keep_written_ranges() {
    let fixture = Fixture::new();
    let source = "@#define helper = \"pkg.sub.foo\"\n@#if false\nexternal_function(name=dormant.foo);\n@#endif\nexternal_function(name=@{helper},first_deriv_provided);";
    let root = fixture.file("model.mod", source);
    let helper = fixture.file("+pkg/+sub/foo.m", "% helper");
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = named(&rows, "pkg.sub.foo");
    assert_path(row, &helper);
    assert_eq!(
        &source[row.named_in.start as usize..row.named_in.end as usize],
        "@{helper}"
    );
}

#[test]
fn include_search_paths_find_package_helpers_and_directories_are_not_files() {
    let fixture = Fixture::new();
    let search = fixture.0.join("search");
    fs::create_dir(&search).unwrap();
    let source = format!(
        "@#includepath \"{}\"\nexternal_function(name=pkg.foo,first_deriv_provided=pkg.jac);",
        search.to_string_lossy().replace('\\', "/")
    );
    let root = fixture.file("main/model.mod", &source);
    let helper = fixture.file("search/+pkg/foo.m", "% package helper");
    fs::create_dir(search.join("+pkg/jac.m")).unwrap();
    let mut workspace = Workspace::new();
    workspace.load_from_disk(&root).unwrap();
    let rows = records(&mut workspace, &root);
    assert_path(named(&rows, "pkg.foo"), &helper);
    assert!(named(&rows, "pkg.jac").path.is_none());
}

#[test]
fn map_only_package_discovery_uses_supplied_files_and_keeps_full_missing_names() {
    let fixture = Fixture::new();
    let source = "external_function(name=pkg.foo); var y; model; y=0; end;";
    let root = fixture.file("model.mod", source);
    let helper = fixture.file("+pkg/foo.m", "% present on disk");
    let key = root.to_string_lossy().to_string();
    let mut files = HashMap::from([(key.clone(), source.to_string())]);
    let missing =
        dynare_workspace_diagnose(Some(&files), Some(std::slice::from_ref(&key)), None).unwrap();
    let row = missing["roots"][0]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["code"] == "W160")
        .unwrap();
    assert!(row["message"].as_str().unwrap().contains("'pkg.foo'"));
    assert_eq!(
        row["end_column"].as_u64().unwrap() - row["column"].as_u64().unwrap(),
        7
    );
    files.insert(
        helper.to_string_lossy().to_string(),
        "% supplied helper".into(),
    );
    let resolved =
        dynare_workspace_diagnose(Some(&files), Some(std::slice::from_ref(&key)), None).unwrap();
    assert!(!resolved["roots"][0]["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .any(|row| row["code"] == "W160"));
}

#[test]
fn qualified_expression_call_ranges_cover_trivia_and_macro_origins() {
    let fixture = Fixture::new();
    let helper = fixture.file("+pkg/+sub/foo.m", "% helper");
    for (expression, prefix, written) in [
        ("pkg . sub . foo(1)", "", "pkg . sub . foo"),
        (
            "@{helper}(1)",
            "@#define helper = \"pkg.sub.foo\"\n",
            "@{helper}",
        ),
    ] {
        let source =
            format!("{prefix}var y; model; y=0; end; steady_state_model; y={expression}; end;");
        let root = fixture.file("model.mod", &source);
        let mut workspace = Workspace::new();
        workspace.load_from_disk(&root).unwrap();
        let rows = records(&mut workspace, &root);
        assert_eq!(rows.len(), 1, "{rows:?}");
        let row = named(&rows, "pkg.sub.foo");
        assert_path(row, &helper);
        assert_eq!(
            &source[row.named_in.start as usize..row.named_in.end as usize],
            written
        );
    }
}

#[test]
fn mcp_related_files_keep_complete_names_and_overlay_keys() {
    let root = "C:/qualified-companions/model.mod";
    let source = "external_function(name=pkg.sub.foo,first_deriv_provided=pkg.jac);";
    let files = HashMap::from([
        (root.into(), source.into()),
        (
            "C:/qualified-companions/+pkg/+sub/foo.m".into(),
            "% foo".into(),
        ),
        ("C:/qualified-companions/+pkg/jac.m".into(), "% jac".into()),
    ]);
    let rows = dynare_related_files(source, Some(root), Some(&files));
    let rows = rows.as_array().unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (name, path) in [
        ("pkg.sub.foo", "C:/qualified-companions/+pkg/+sub/foo.m"),
        ("pkg.jac", "C:/qualified-companions/+pkg/jac.m"),
    ] {
        let row = rows.iter().find(|row| row["filename"] == name).unwrap();
        assert_eq!(row["kind"], "helper_m");
        assert_eq!(row["resolved"], true);
        assert_eq!(row["path"], path);
    }
}

#[tokio::test]
async fn lsp_document_links_cover_full_qualified_names_in_utf16() {
    let fixture = Fixture::new();
    let source = "/* 🚀 */ external_function(name=pkg . sub . foo,first_deriv_provided=pkg.jac);";
    let root = fixture.file("model.mod", source);
    let foo = fixture.file("+pkg/+sub/foo.m", "% foo");
    let jac = fixture.file("+pkg/jac.m", "% jac");
    let uri = Url::from_file_path(&root).unwrap();
    let (service, _socket) = new_service();
    let server = service.inner();
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version: 1,
                text: source.into(),
            },
        })
        .await;
    let rows = server
        .document_link(DocumentLinkParams {
            text_document: TextDocumentIdentifier { uri },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    for (name, path) in [("pkg . sub . foo", foo), ("pkg.jac", jac)] {
        let target = Url::from_file_path(fs::canonicalize(path).unwrap()).unwrap();
        let row = rows
            .iter()
            .find(|row| row.target.as_ref() == Some(&target))
            .unwrap();
        let start = source[..source.find(name).unwrap()].encode_utf16().count() as u32;
        assert_eq!(
            row.range,
            Range::new(
                Position::new(0, start),
                Position::new(0, start + name.len() as u32)
            )
        );
    }
}
