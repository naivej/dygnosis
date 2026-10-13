//! Actual Rename requests: complete ownership, binding, and source guards.

use dygnosis::server::{new_service, Backend};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower_lsp::lsp_types::*;
use tower_lsp::LanguageServer;

static SEQUENCE: AtomicUsize = AtomicUsize::new(0);
const MODEL: &str = "var y;\nparameters p;\np=0.9;\nmodel;\ny=p*y(-1);\nend;\n";

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "dygnosis-rename-{}-{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn write(&self, name: &str, text: &str) -> Url {
        let path = self.0.join(name);
        fs::write(&path, text).unwrap();
        Url::from_file_path(path).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
async fn open(server: &Backend, uri: &Url, text: &str, version: i32) {
    server
        .did_open(DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: uri.clone(),
                language_id: "dynare".into(),
                version,
                text: text.into(),
            },
        })
        .await;
}
fn position(uri: &Url, text: &str, name: &str) -> TextDocumentPositionParams {
    let byte = if dygnosis::refs::is_legal_ident(name) {
        dygnosis::refs::occurrences(text, name)
            .first()
            .unwrap()
            .start
    } else {
        text.find(name).unwrap() as u32
    };
    let pos = dygnosis::span::LineIndex::new(text).position_utf16(text, byte);
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier { uri: uri.clone() },
        position: Position::new(pos.line, pos.character),
    }
}
async fn rename(
    server: &Backend,
    uri: &Url,
    text: &str,
    old: &str,
    new: &str,
) -> Option<WorkspaceEdit> {
    server
        .rename(RenameParams {
            text_document_position: position(uri, text, old),
            new_name: new.into(),
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap_or_else(|error| {
            assert!(
                !error.message.trim().is_empty(),
                "refused Rename needs a reason"
            );
            None
        })
}
fn edits(edit: WorkspaceEdit) -> HashMap<Url, (Option<i32>, Vec<TextEdit>)> {
    assert!(
        edit.changes.is_none(),
        "ordinary rename must retain open document versions"
    );
    let DocumentChanges::Edits(documents) = edit.document_changes.unwrap() else {
        panic!("text edits")
    };
    documents
        .into_iter()
        .map(|row| {
            (
                row.text_document.uri,
                (
                    row.text_document.version,
                    row.edits
                        .into_iter()
                        .map(|edit| match edit {
                            OneOf::Left(edit) => edit,
                            _ => panic!("plain edit"),
                        })
                        .collect(),
                ),
            )
        })
        .collect()
}
fn apply(text: &str, edits: &[TextEdit]) -> String {
    let index = dygnosis::span::LineIndex::new(text);
    let mut spans: Vec<_> = edits
        .iter()
        .map(|edit| {
            let start = index.offset_utf16(
                text,
                dygnosis::span::Position {
                    line: edit.range.start.line,
                    character: edit.range.start.character,
                },
            );
            let end = index.offset_utf16(
                text,
                dygnosis::span::Position {
                    line: edit.range.end.line,
                    character: edit.range.end.character,
                },
            );
            (start, end, &edit.new_text)
        })
        .collect();
    spans.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut result = text.to_owned();
    for (start, end, replacement) in spans {
        result.replace_range(start as usize..end as usize, replacement);
    }
    result
}

// Guards References including filename/option/native slots or ignoring its
// declaration switch when the ordinary Rename binding is extracted.
#[tokio::test]
async fn ordinary_references_select_accepted_sites_and_semantic_tags() {
    let fixture = Fixture::new();
    let text = "var y;parameters order;order=0.9;model;[name='y',mcp='y > 0'] y=order;end;\nstoch_simul(order=1) y;\nload_params_and_steady_state(order);\nverbatim;\norder=2;y=3;\nend;\n// y order\n";
    let root = fixture.write("references.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    for (name, include_declaration, expected) in [
        ("order;", true, vec!["order;", "order=0.9", "y=order"]),
        ("order;", false, vec!["order=0.9", "y=order"]),
        ("y;", false, vec!["y > 0", "y=order", " y;\\nload"]),
    ] {
        let locations = service
            .inner()
            .references(ReferenceParams {
                text_document_position: position(&root, text, name),
                context: ReferenceContext {
                    include_declaration,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        let index = dygnosis::span::LineIndex::new(text);
        let starts: Vec<_> = locations
            .iter()
            .map(|site| {
                index.offset_utf16(
                    text,
                    dygnosis::span::Position {
                        line: site.range.start.line,
                        character: site.range.start.character,
                    },
                ) as usize
            })
            .collect();
        let expected: Vec<_> = expected
            .into_iter()
            .map(|marker| {
                let marker = marker.replace("\\n", "\n");
                let offset = usize::from(marker.starts_with("y=order") && name == "order;") * 2
                    + usize::from(marker.starts_with(" y;"));
                text.find(&marker).unwrap() + offset
            })
            .collect();
        assert_eq!(
            starts, expected,
            "name={name}, include_declaration={include_declaration}"
        );
    }
}

// Shared local includes can be renamed when every owner has the same verified
// declaration, definition and aggregate scope. Edits retain document versions.
#[tokio::test]
async fn rename_shared_local_binding_updates_compatible_model_owners() {
    let fixture = Fixture::new();
    let declaration = fixture.write("decl.inc", "model_local_variable helper;\n");
    let definition_text = "#helper=1;\n";
    let definition = fixture.write("def.inc", definition_text);
    let a_text = "var y;\n@#include \"decl.inc\"\nmodel;\n@#include \"def.inc\"\ny=helper;end;\n";
    let b_text = a_text
        .replace("var y", "var z")
        .replace("y=helper", "z=helper");
    let a = fixture.write("a.mod", a_text);
    let b = fixture.write("b.mod", &b_text);
    let (service, _socket) = new_service();
    open(service.inner(), &a, a_text, 1).await;
    open(service.inner(), &b, &b_text, 2).await;
    open(service.inner(), &definition, definition_text, 3).await;
    let changes = edits(
        rename(
            service.inner(),
            &definition,
            definition_text,
            "helper",
            "shared_helper",
        )
        .await
        .unwrap(),
    );
    assert_eq!(changes[&a].0, Some(1));
    assert_eq!(changes[&b].0, Some(2));
    assert_eq!(changes[&definition].0, Some(3));
    assert!(changes.contains_key(&declaration));
    assert_eq!(
        apply(a_text, &changes[&a].1),
        a_text.replace("y=helper", "y=shared_helper")
    );
    assert_eq!(
        apply(&b_text, &changes[&b].1),
        b_text.replace("z=helper", "z=shared_helper")
    );
}

// Guards Prepare succeeding while Rename drops a supported document entirely.
#[tokio::test]
async fn rename_supported_roots_and_untitled_use_one_decision() {
    let fixture = Fixture::new();
    for uri in [
        fixture.write("root.mod", MODEL),
        fixture.write("root.dyn", MODEL),
        Url::parse("untitled:Untitled-1").unwrap(),
        Url::parse("untitled:Unsaved.mod").unwrap(),
    ] {
        let (service, _socket) = new_service();
        open(service.inner(), &uri, MODEL, 7).await;
        assert!(service
            .inner()
            .prepare_rename(position(&uri, MODEL, "p;"))
            .await
            .unwrap()
            .is_some());
        let changes = edits(
            rename(service.inner(), &uri, MODEL, "p;", "rho")
                .await
                .unwrap(),
        );
        assert_eq!(changes[&uri].0, Some(7));
        assert_eq!(
            apply(MODEL, &changes[&uri].1),
            MODEL
                .replace(" p;", " rho;")
                .replace("p=", "rho=")
                .replace("=p", "=rho")
        );
    }
}

// Guards a declaration changing while a saved/open use with another suffix stays old.
#[tokio::test]
async fn rename_owned_include_names_use_saved_source_or_open_overlay() {
    let fixture = Fixture::new();
    for suffix in ["dyn", "inc", "txt", "m"] {
        for opened in [false, true] {
            let child_text = if opened {
                "y=p*y(-1)+p;\n"
            } else {
                "y=p*y(-1);\n"
            };
            let child = fixture.write(&format!("child.{suffix}"), "y=p*y(-1);\n");
            let root_text = format!(
                "var y;\nparameters p;\np=0.9;\nmodel;\n@#include \"child.{suffix}\"\nend;\n"
            );
            let root = fixture.write("root.mod", &root_text);
            let (service, _socket) = new_service();
            open(service.inner(), &root, &root_text, 1).await;
            if opened {
                open(service.inner(), &child, child_text, 4).await;
            }
            assert!(service
                .inner()
                .prepare_rename(position(&root, &root_text, "p;"))
                .await
                .unwrap()
                .is_some());
            let changes = edits(
                rename(service.inner(), &root, &root_text, "p;", "rho")
                    .await
                    .unwrap(),
            );
            assert_eq!(changes.len(), 2);
            assert_eq!(changes[&child].0, opened.then_some(4));
            assert_eq!(
                apply(child_text, &changes[&child].1),
                child_text.replace('p', "rho")
            );
        }
    }
}

// Guards an unopened include edit following canonicalize instead of the
// opened root. Windows 8.3 temp paths and directory links are different URLs.
#[cfg(any(unix, windows))]
#[tokio::test]
async fn rename_unopened_include_keeps_the_open_roots_path_spelling() {
    let fixture = Fixture::new();
    let real = fixture.0.join("real");
    fs::create_dir(&real).unwrap();
    let child_text = "y=p;\n";
    fs::write(real.join("child.inc"), child_text).unwrap();
    let root_text = "var y;parameters p;model;\n@#include \"child.inc\"\nend;\n";
    fs::write(real.join("root.mod"), root_text).unwrap();
    let alias = fixture.0.join("alias");
    link_directory(&alias, &real);
    let root = Url::from_file_path(alias.join("root.mod")).unwrap();
    let child = Url::from_file_path(alias.join("child.inc")).unwrap();
    let canonical =
        Url::from_file_path(fs::canonicalize(alias.join("child.inc")).unwrap()).unwrap();
    assert_ne!(
        child, canonical,
        "the directory link must differ from the canonical include path"
    );
    let (service, _socket) = new_service();
    open(service.inner(), &root, root_text, 1).await;
    let changes = edits(
        rename(service.inner(), &root, root_text, "p;", "rho")
            .await
            .unwrap(),
    );
    assert!(
        changes.contains_key(&root) && changes.contains_key(&child),
        "open root {root}; alias include {child}; canonical include {canonical}; edit URIs {:?}",
        changes.keys().map(Url::as_str).collect::<Vec<_>>()
    );
    assert_eq!(apply(child_text, &changes[&child].1), "y=rho;\n");
}

fn link_directory(link: &std::path::Path, target: &std::path::Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let result = std::process::Command::new("cmd.exe")
            .creation_flags(0x08000000)
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "directory junction failed: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

// Guards edits spreading to another root solely because its name has equal spelling.
#[tokio::test]
async fn rename_independent_equal_spellings_stay_separate() {
    let fixture = Fixture::new();
    let first = fixture.write("first.mod", MODEL);
    let second = fixture.write("second.mod", MODEL);
    let (service, _socket) = new_service();
    open(service.inner(), &first, MODEL, 1).await;
    open(service.inner(), &second, MODEL, 1).await;
    let changes = edits(
        rename(service.inner(), &first, MODEL, "p;", "rho")
            .await
            .unwrap(),
    );
    assert_eq!(changes.len(), 1);
    assert!(changes.contains_key(&first));
}

// Guards ordinary/local collisions, including one that is only in another owner.
#[tokio::test]
async fn rename_rejects_ordinary_and_local_collisions_and_keyword_changes() {
    let fixture = Fixture::new();
    for text in [
        MODEL.replace("parameters p;", "parameters p q;"),
        MODEL.replace("y=p", "#q=1;\ny=p"),
    ] {
        let root = fixture.write("root.mod", &text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, &text, 1).await;
        for new in ["q", "end", "steady", "Ln", "stoch_simul", "p"] {
            assert!(
                rename(service.inner(), &root, &text, "p", new)
                    .await
                    .is_none(),
                "unsafe replacement {new}"
            );
        }
    }
}

// Guards PAC discount references being omitted or blocking the complete Rename,
// while native MATLAB strings and comments keep their text.
#[tokio::test]
async fn rename_and_references_include_pac_discount_parameter() {
    let fixture = Fixture::new();
    for option in ["discount", "Discount"] {
        let text = format!(
            "parameters beta;\nbeta=.9;\npac_model(model_name=pac_y, {option}=beta);\nuser_params={{'beta'}};\n// beta\n"
        );
        let root = fixture.write(&format!("pac_{option}.mod"), &text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, &text, 1).await;
        let expected: Vec<_> = ["beta;", "beta=", "beta);"]
            .into_iter()
            .map(|marker| {
                let start = position(&root, &text, marker).position;
                Location::new(
                    root.clone(),
                    Range::new(start, Position::new(start.line, start.character + 4)),
                )
            })
            .collect();
        let references = service
            .inner()
            .references(ReferenceParams {
                text_document_position: position(&root, &text, "beta;"),
                context: ReferenceContext {
                    include_declaration: true,
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            })
            .await
            .unwrap()
            .unwrap();
        assert_eq!(references, expected, "PAC option {option}");
        assert!(service
            .inner()
            .prepare_rename(position(&root, &text, "beta);"))
            .await
            .unwrap()
            .is_some());
        let changes = edits(
            rename(service.inner(), &root, &text, "beta);", "betta")
                .await
                .expect("complete PAC discount Rename"),
        );
        assert_eq!(changes[&root].0, Some(1));
        let expected_text = text
            .replace("parameters beta;", "parameters betta;")
            .replace("\nbeta=", "\nbetta=")
            .replace(&format!("{option}=beta)"), &format!("{option}=betta)"));
        assert_eq!(apply(&text, &changes[&root].1), expected_text);
    }
}

// Guards the new discount binding from making generated names editable or
// treating a PAC model name as an ordinary parameter reference.
#[tokio::test]
async fn rename_pac_discount_keeps_generated_and_unbound_slot_guards() {
    let fixture = Fixture::new();
    for text in [
        "parameters beta; pac_model(model_name=pac_y, discount=@{\"beta\"});\n",
        "parameters beta; pac_model(model_name=beta, discount=beta);\n",
    ] {
        let root = fixture.write("unproved_pac.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        let refusal = service
            .inner()
            .prepare_rename(position(&root, text, "beta;"))
            .await
            .expect_err("unproved PAC name must withhold Rename");
        assert!(!refusal.message.is_empty());
        assert!(rename(service.inner(), &root, text, "beta;", "betta")
            .await
            .is_none());
    }
}

// Guards safe shared bindings being lost and incompatible shared bindings being edited.
#[tokio::test]
async fn rename_shared_include_requires_the_same_written_declaration() {
    let fixture = Fixture::new();
    for compatible in [true, false] {
        let declarations = fixture.write("declarations.inc", "parameters p;\np=0.9;\n");
        let shared = fixture.write("equations.data", "y=p*y(-1);\n");
        let declaration = if compatible {
            "@#include \"declarations.inc\"\n"
        } else {
            "parameters p;\np=0.9;\n"
        };
        let text = format!("var y;\n{declaration}model;\n@#include \"equations.data\"\nend;\n");
        let first = fixture.write("first.mod", &text);
        let second = fixture.write("second.mod", &text);
        let (service, _socket) = new_service();
        open(service.inner(), &first, &text, 1).await;
        open(service.inner(), &second, &text, 2).await;
        open(service.inner(), &shared, "y=p*y(-1);\n", 3).await;
        let pos = position(&shared, "y=p*y(-1);\n", "p");
        assert_eq!(
            service
                .inner()
                .prepare_rename(pos)
                .await
                .map(|prepared| prepared.is_some())
                .unwrap_or_else(|error| {
                    assert!(!error.message.is_empty());
                    false
                }),
            compatible
        );
        let result = rename(service.inner(), &shared, "y=p*y(-1);\n", "p", "rho").await;
        assert_eq!(result.is_some(), compatible);
        if let Some(result) = result {
            let changes = edits(result);
            assert!(changes.contains_key(&declarations));
            assert_eq!(apply("y=p*y(-1);\n", &changes[&shared].1), "y=rho*y(-1);\n");
        }
    }
}

// Guards a compatible owner's collision being missed when Rename starts in the first root.
#[tokio::test]
async fn rename_checks_collisions_in_every_shared_owner() {
    let fixture = Fixture::new();
    fixture.write("decl.inc", "parameters p;\n");
    let first_text = "var y;\n@#include \"decl.inc\"\nmodel;y=p;end;\n";
    let second_text = "var y;\nparameters rho;\n@#include \"decl.inc\"\nmodel;y=p+rho;end;\n";
    let first = fixture.write("first.mod", first_text);
    let second = fixture.write("second.mod", second_text);
    let (service, _socket) = new_service();
    open(service.inner(), &first, first_text, 1).await;
    open(service.inner(), &second, second_text, 1).await;
    assert!(rename(service.inner(), &first, first_text, "p;", "rho")
        .await
        .is_none());
}

// Guards duplicate written macro edits and edits into interpolated/generated names.
#[tokio::test]
async fn rename_deduplicates_macro_copies_and_withholds_generated_targets() {
    let fixture = Fixture::new();
    let text = "var y;parameters p;model;\n@#for n in 1:2\ny=p*y(-1);\n@#endfor\nend;\n";
    let root = fixture.write("root.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    let changes = edits(
        rename(service.inner(), &root, text, "p;", "rho")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply(text, &changes[&root].1),
        text.replace("parameters p", "parameters rho")
            .replace("y=p", "y=rho")
    );
    let generated = "var y;parameters p;model;y=@{\"p\"};end;\n";
    let root = fixture.write("generated.mod", generated);
    open(service.inner(), &root, generated, 1).await;
    assert!(!service
        .inner()
        .prepare_rename(position(&root, generated, "p;"))
        .await
        .expect_err("refused Rename must explain its cause")
        .message
        .is_empty());
    assert!(rename(service.inner(), &root, generated, "p;", "rho")
        .await
        .is_none());
}

// Guards stale saved-file ranges and read-only targets becoming partial edits.
#[tokio::test]
async fn rename_rechecks_saved_sources_and_requires_all_targets_editable() {
    let fixture = Fixture::new();
    let child = fixture.write("child.inc", "y=p;\n");
    let text = "var y;parameters p;model;\n@#include \"child.inc\"\nend;\n";
    let root = fixture.write("root.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    assert!(service
        .inner()
        .prepare_rename(position(&root, text, "p;"))
        .await
        .unwrap()
        .is_some());
    let path = child.to_file_path().unwrap();
    fs::write(&path, "// changed\ny=p+p;\n").unwrap();
    let changes = edits(
        rename(service.inner(), &root, text, "p;", "rho")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply("// changed\ny=p+p;\n", &changes[&child].1),
        "// changed\ny=rho+rho;\n"
    );
    let original_permissions = fs::metadata(&path).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions.clone()).unwrap();
    assert!(rename(service.inner(), &root, text, "p;", "rho")
        .await
        .is_none());
    fs::set_permissions(&path, original_permissions).unwrap();
    fs::remove_file(path).unwrap();
    assert!(!service
        .inner()
        .prepare_rename(position(&root, text, "p;"))
        .await
        .expect_err("refused Rename must explain its cause")
        .message
        .is_empty());
    assert!(rename(service.inner(), &root, text, "p;", "rho")
        .await
        .is_none());
}

// Guards source-map ranges being sent as scalar columns after a supplementary character.
#[tokio::test]
async fn rename_valid_source_keeps_utf16_ranges_and_non_symbol_text() {
    let fixture = Fixture::new();
    let text = "parameters p (long_name='😀😀'); var y; model;y=p;end;\n";
    let root = fixture.write("utf16.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    let changes = edits(
        rename(service.inner(), &root, text, "y;", "z")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply(text, &changes[&root].1),
        text.replace("var y", "var z").replace("model;y", "model;z")
    );
    let text = "var y;parameters order;order=0.9;model;y=order;end;stoch_simul(order=1) y;\n";
    let root = fixture.write("options.mod", text);
    open(service.inner(), &root, text, 1).await;
    let changes = edits(
        rename(service.inner(), &root, text, "order;", "rho")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply(text, &changes[&root].1),
        "var y;parameters rho;rho=0.9;model;y=rho;end;stoch_simul(order=1) y;\n"
    );
}

// Guards a lexical spelling match in a filename slot becoming a symbol edit.
#[tokio::test]
async fn rename_withholds_unproven_name_slots() {
    let fixture = Fixture::new();
    let text = "var y;parameters p;p=0.9;model;y=p;end;load_params_and_steady_state(p);\n";
    let root = fixture.write("filename.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    assert!(rename(service.inner(), &root, text, "p;", "rho")
        .await
        .is_none());
}

// Guards the native capture omitting an unedited owner or macro-only input
// whose version still determines the shared declaration/binding decision.
#[tokio::test]
async fn rename_capture_retains_all_affected_model_inputs() {
    let fixture = Fixture::new();
    let flags = fixture.write("flags.data", "@#define variant=1\n");
    let declaration = fixture.write("declaration.inc", "parameters p;\n");
    let shared = fixture.write("equation.inc", "y=p;\n");
    let text = "@#include \"flags.data\"\n@#include \"declaration.inc\"\nvar y;model;\n@#include \"equation.inc\"\nend;\n";
    let first = fixture.write("first.mod", text);
    let second = fixture.write("second.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &first, text, 1).await;
    open(service.inner(), &second, text, 2).await;
    open(service.inner(), &shared, "y=p;\n", 3).await;
    let params = RenameParams {
        text_document_position: position(&shared, "y=p;\n", "p"),
        new_name: "rho".into(),
        work_done_progress_params: WorkDoneProgressParams::default(),
    };
    let captured = service
        .inner()
        .execute_command(ExecuteCommandParams {
            command: "dynare/captureRename".into(),
            arguments: vec![serde_json::to_value(params).unwrap()],
            work_done_progress_params: WorkDoneProgressParams::default(),
        })
        .await
        .unwrap()
        .unwrap();
    assert_eq!(captured["schema_version"], 1);
    let changes = edits(serde_json::from_value(captured["edit"].clone()).unwrap());
    assert!(changes.contains_key(&declaration) && changes.contains_key(&shared));
    assert!(
        !changes.contains_key(&first)
            && !changes.contains_key(&second)
            && !changes.contains_key(&flags)
    );
    for uri in [&first, &second, &flags, &declaration, &shared] {
        assert!(
            captured["sources"]
                .as_array()
                .unwrap()
                .iter()
                .any(|source| source["uri"].as_str() == Some(uri.as_str())),
            "missing input {uri}"
        );
    }
}

// Guards a retained predetermined-variable use blocking the complete Rename,
// or remaining old after its declaration and equation uses change.
#[tokio::test]
async fn rename_includes_predetermined_variable_name_slots() {
    let fixture = Fixture::new();
    let text = "var y;\npredetermined_variables y;\nmodel;\ny=y(-1);\nend;\n";
    let root = fixture.write("predetermined.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    assert!(service
        .inner()
        .prepare_rename(position(&root, text, "y;\nmodel"))
        .await
        .unwrap()
        .is_some());
    let changes = edits(
        rename(service.inner(), &root, text, "y;\nmodel", "z")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply(text, &changes[&root].1),
        "var z;\npredetermined_variables z;\nmodel;\nz=z(-1);\nend;\n"
    );
}

// Guards the accepted homotopy/OSR target names being omitted from the edit,
// and an accepted prior-copy source being mistaken for an unrelated name slot.
#[tokio::test]
async fn rename_includes_homotopy_osr_bounds_and_prior_copy_sources() {
    let fixture = Fixture::new();
    for (filename, text, expected, selected) in [
        ("homotopy.mod", "var y;parameters p;p=.9;model;y=p;end;homotopy_setup;p,.5,.9;end;\n", "var y;parameters rho;rho=.9;model;y=rho;end;homotopy_setup;rho,.5,.9;end;\n", "p,.5"),
        ("osr.mod", "var y;parameters p;p=.9;model;y=p;end;osr_params p;osr_params_bounds;p,0,1;end;\n", "var y;parameters rho;rho=.9;model;y=rho;end;osr_params rho;osr_params_bounds;rho,0,1;end;\n", "p,0"),
        ("prior_copy.mod", "var y;parameters p q;p=.9;model;y=p;end;q.prior=p.prior;\n", "var y;parameters rho q;rho=.9;model;y=rho;end;q.prior=rho.prior;\n", "p.prior"),
    ] {
        let root = fixture.write(filename, text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        assert!(service.inner().prepare_rename(position(&root, text, selected)).await.unwrap().is_some(), "Prepare {filename}");
        let changes = edits(rename(service.inner(), &root, text, selected, "rho").await.unwrap());
        assert_eq!(apply(text, &changes[&root].1), expected, "Rename {filename}");
    }
}

// Guards identifier-only edits leaving semantic tag references stale. The
// unchanged non-BMP display label also proves tag edits use UTF-16 positions.
#[tokio::test]
async fn rename_updates_verified_semantic_tag_references() {
    let fixture = Fixture::new();
    for (text, selected, expected) in [
        ("var y;model;[endogenous='y'] y=y(-1);end;\n", "y;", "var z;model;[endogenous='z'] z=z(-1);end;\n"),
        ("var y;model;[mcp='y > 0'] y=0;end;\n", "y >", "var z;model;[mcp='z > 0'] z=0;end;\n"),
        ("var y(long_name='y');model;[name='😀 y',note='y',endogenous='y',mcp='  y  >  0'] y=1;end;\n", "y',mcp", "var z(long_name='y');model;[name='😀 y',note='y',endogenous='z',mcp='  z  >  0'] z=1;end;\n"),
        ("model;[endogenous='y'] y=0;end;\n", "y']", "model;[endogenous='z'] z=0;end;\n"),
        ("@#define bound=0\nvar y;model;[mcp='y  >  @{bound}'] y=0;end;\n", "y  >", "@#define bound=0\nvar z;model;[mcp='z  >  @{bound}'] z=0;end;\n"),
        ("var y;model;\n@#for i in [1,2]\n[endogenous='y',mcp='y>0'] y=0;\n@#endfor\nend;\n", "y;", "var z;model;\n@#for i in [1,2]\n[endogenous='z',mcp='z>0'] z=0;\n@#endfor\nend;\n"),
    ] {
        let root = fixture.write("semantic_tag.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        assert!(service
            .inner()
            .prepare_rename(position(&root, text, selected))
            .await
            .unwrap()
            .is_some(), "Prepare {text}");
        let changes = edits(rename(service.inner(), &root, text, selected, "z").await.unwrap_or_else(|| panic!("Rename {text}")));
        assert_eq!(apply(text, &changes[&root].1), expected);
    }
}

// Guards implicit tag-owned declaration collisions and generated semantic
// identifiers. A literal mcp bound does not prove an interpolated left side.
#[tokio::test]
async fn rename_refuses_unproved_tag_names_and_tag_declaration_collisions() {
    let fixture = Fixture::new();
    for text in [
        "@#define symbol=\"y\"\nvar y;model;[endogenous='@{symbol}'] y=0;end;\n",
        "@#define symbol=\"y\"\nvar y;model;[mcp='@{symbol} > 0'] y=0;end;\n",
        "var y;model;[endogenous=\"y\"] y=0;end;\n",
    ] {
        let root = fixture.write("unproved.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        assert!(!service
            .inner()
            .prepare_rename(position(&root, text, "y;"))
            .await
            .expect_err("refused Rename must explain its cause")
            .message
            .is_empty());
        assert!(rename(service.inner(), &root, text, "y;", "z")
            .await
            .is_none());
    }
    let text = "var y;model;[endogenous='z'] z=y;y=1;end;\n";
    let root = fixture.write("tag_collision.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    assert!(rename(service.inner(), &root, text, "y;", "z")
        .await
        .is_none());
}

// Guards compatible literal tag includes requiring all owner edits, and
// independent declarations being mistaken for the same shared binding.
#[tokio::test]
async fn rename_semantic_tag_include_requires_compatible_owners() {
    let fixture = Fixture::new();
    let declaration = fixture.write("declaration.txt", "var y;\n");
    let tag_text = "[endogenous='y',mcp='y > 0'] y=0;\n";
    let shared = fixture.write("tag.inc", tag_text);
    for compatible in [true, false] {
        let text = if compatible {
            "@#include \"declaration.txt\"\nmodel;\n@#include \"tag.inc\"\nend;\n"
        } else {
            "var y;model;\n@#include \"tag.inc\"\nend;\n"
        };
        let first = fixture.write("first.mod", text);
        let second = fixture.write("second.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &first, text, 1).await;
        open(service.inner(), &second, text, 1).await;
        open(service.inner(), &shared, tag_text, 1).await;
        assert_eq!(
            service
                .inner()
                .prepare_rename(position(&shared, tag_text, "y',mcp"))
                .await
                .map(|prepared| prepared.is_some())
                .unwrap_or_else(|error| {
                    assert!(!error.message.is_empty());
                    false
                }),
            compatible
        );
        let renamed = rename(service.inner(), &shared, tag_text, "y',mcp", "z").await;
        if compatible {
            let changes = edits(renamed.unwrap());
            assert_eq!(
                apply(tag_text, &changes[&shared].1),
                "[endogenous='z',mcp='z > 0'] z=0;\n"
            );
            assert_eq!(apply("var y;\n", &changes[&declaration].1), "var z;\n");
        } else {
            assert!(renamed.is_none());
        }
    }
}

// Guards a shared physical literal executing as both a semantic tag reference
// and a display label. Renaming it would change a label outside this binding.
#[tokio::test]
async fn rename_refuses_tag_literal_with_incompatible_macro_roles() {
    let fixture = Fixture::new();
    for (text, selected) in [
        ("var y;model;\n@#for key in [\"endogenous\",\"name\"]\n[@{key}='y'] y=0;\n@#endfor\nend;\n", "y;"),
        ("var y;model;\n@#for key in [\"endogenous\",\"name\"]\n@#if key == \"name\"\n@#define prefix=\"label_\"\n@#else\n@#define prefix=\"\"\n@#endif\n[@{key}='@{prefix}y'] y=0;\n@#endfor\nend;\n", "y;"),
        ("var aa;model;\n@#for key in [\"endogenous\",\"name\"]\n@#if key == \"name\"\n@#define prefix=\"a\"\n@#else\n@#define prefix=\"\"\n@#endif\n[@{key}='@{prefix}aa'] aa=0;\n@#endfor\nend;\n", "aa;"),
    ] {
        // The same macro structure is editable when every execution is the
        // semantic reference. This rules out invalid syntax as the refusal.
        let control = text.replace("[\"endogenous\",\"name\"]", "[\"endogenous\",\"endogenous\"]");
        let control_uri = fixture.write("tag_control.mod", &control);
        let (control_service, _control_socket) = new_service();
        open(control_service.inner(), &control_uri, &control, 1).await;
        assert!(rename(control_service.inner(), &control_uri, &control, selected, "z").await.is_some());
        let root = fixture.write("tag_roles.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        assert!(!service.inner().prepare_rename(position(&root, text, selected)).await.expect_err("refused Rename must explain its cause").message.is_empty());
        assert!(rename(service.inner(), &root, text, selected, "z").await.is_none());
    }
}

// Guards the original semantic-tag regression against the pinned official
// Transform stage, using the exact output of actual Rename requests.
#[tokio::test]
async fn rename_applied_semantic_tags_pass_pinned_transform() {
    let binary = PathBuf::from(r"C:\dynare\7.2\preprocessor\dynare-preprocessor.exe");
    if !binary.is_file() {
        eprintln!("skipping honesty: pinned Dynare 7.2 is absent");
        return;
    }
    let fixture = Fixture::new();
    for text in [
        "var y;model;[endogenous='y'] y=y(-1);end;\n",
        "var y;model;[mcp='y > 0'] y=0;end;\n",
        "model;[endogenous='y'] y=0;end;\n",
        "@#define bound=0\nvar y;model;[mcp='y  >  @{bound}'] y=0;end;\n",
    ] {
        let root = fixture.write("official.mod", text);
        let (service, _socket) = new_service();
        open(service.inner(), &root, text, 1).await;
        let selected = if text.starts_with("model") {
            "y']"
        } else {
            "y;"
        };
        let changes = edits(
            rename(service.inner(), &root, text, selected, "z")
                .await
                .unwrap(),
        );
        let applied = apply(text, &changes[&root].1);
        for source in [text, &applied] {
            let result = dygnosis::run_preprocessor(
                source,
                &binary,
                None,
                std::time::Duration::from_secs(30),
                dygnosis::JsonStage::Transform,
            );
            assert!(
                result.success,
                "Transform rejected {source}\n{}\n{}",
                result.raw_stdout, result.raw_stderr
            );
        }
    }
}

// Guards semantic-tag refusal expanding to unrelated equation/display labels.
#[tokio::test]
async fn rename_preserves_unrelated_equation_and_long_name_labels() {
    let fixture = Fixture::new();
    let text = "var y (long_name='y');model;[name='y',note='y'] y=y(-1);end;\n";
    let root = fixture.write("labels.mod", text);
    let (service, _socket) = new_service();
    open(service.inner(), &root, text, 1).await;
    assert!(service
        .inner()
        .prepare_rename(position(&root, text, "y"))
        .await
        .unwrap()
        .is_some());
    let changes = edits(
        rename(service.inner(), &root, text, "y", "z")
            .await
            .unwrap(),
    );
    assert_eq!(
        apply(text, &changes[&root].1),
        "var z (long_name='y');model;[name='y',note='y'] z=z(-1);end;\n"
    );
}
