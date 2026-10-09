use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use dygnosis::compare_snapshots::{
    capture_git_snapshot, capture_working_snapshot, compare_captured_snapshots, CapturedSnapshot,
    GitSnapshotInput, ManifestEntry, SnapshotCapture, SnapshotCoordinates, SourceFact,
};
use dygnosis::Workspace;
use serde_json::Value;

fn input(id: &str, root: &str, files: &[(&str, &str)]) -> GitSnapshotInput {
    GitSnapshotInput {
        input_id: id.to_owned(),
        root_file: root.to_owned(),
        repository_uri: "file:///snapshot-test".to_owned(),
        commit: if id == "before" {
            "a".repeat(40)
        } else {
            "b".repeat(40)
        },
        requested_ref: None,
        search_paths: Vec::new(),
        manifest: files
            .iter()
            .map(|(key, _)| {
                (
                    (*key).to_owned(),
                    ManifestEntry {
                        mode: "100644".to_owned(),
                        object_id: "c".repeat(40),
                    },
                )
            })
            .collect(),
        sources: files
            .iter()
            .map(|(key, text)| {
                (
                    (*key).to_owned(),
                    SourceFact::Text {
                        text: (*text).to_owned(),
                    },
                )
            })
            .collect(),
    }
}

fn ready(input: &GitSnapshotInput) -> CapturedSnapshot {
    match capture_git_snapshot(input) {
        SnapshotCapture::Ready(snapshot) => *snapshot,
        SnapshotCapture::NeedsSources(keys) => panic!("unexpected requests {keys:?}"),
        SnapshotCapture::Failure(failure) => {
            panic!("unexpected failure: {}: {}", failure.code, failure.message)
        }
    }
}

fn changed_row(result: &Value) -> &Value {
    result["navigation"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == "/changed_equations/0")
        .unwrap()
}

#[test]
fn deferred_sources_follow_executed_macros_and_never_request_inactive_bodies() {
    let mut snapshot = input("before", "root.mod", &[
        ("root.mod", "@#define pick = \"active\"\n@#include pick\n@#if 0\n@#include \"inactive\"\n@#endif\n"),
        ("active", "@#include \"nested\"\n"),
        ("nested", "var y; model; [name='r'] y=1; end;"),
        ("inactive", "unreadable inactive source"),
    ]);
    let bodies = std::mem::take(&mut snapshot.sources);
    for expected in ["root.mod", "active", "nested"] {
        match capture_git_snapshot(&snapshot) {
            SnapshotCapture::NeedsSources(keys) => assert_eq!(keys, [expected]),
            _ => panic!("expected deferred {expected}"),
        }
        snapshot
            .sources
            .insert(expected.to_owned(), bodies[expected].clone());
    }
    let captured = ready(&snapshot);
    assert_eq!(
        captured
            .sources
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["active", "nested", "root.mod"]
    );
    assert!(!captured.sources.contains_key("inactive"));
}

#[test]
fn candidate_order_uses_tree_paths_dotdot_directories_and_exact_case() {
    let snapshot = input(
        "before",
        "models/main.mod",
        &[
            (
                "models/main.mod",
                "@#includepath \"../shared\"\n@#include \"a.data\"\n",
            ),
            ("shared/a.data", "@#include \"./nested/../b\"\n"),
            ("shared/b", "var y; model; [name='r'] y=1; end;"),
            ("unrelated/a.data", "var wrong; model; wrong=999; end;"),
        ],
    );
    let captured = ready(&snapshot);
    assert_eq!(
        captured
            .sources
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["models/main.mod", "shared/a.data", "shared/b"]
    );
    let mut configured = snapshot.clone();
    configured.sources.insert(
        "models/main.mod".to_owned(),
        SourceFact::Text {
            text: "@#include \"a.data\"\n".to_owned(),
        },
    );
    configured.search_paths = vec!["shared".to_owned(), "unrelated".to_owned()];
    let configured = ready(&configured);
    assert_eq!(
        configured.sources["shared/a.data"],
        captured.sources["shared/a.data"]
    );
    assert_eq!(configured.sources["shared/b"], captured.sources["shared/b"]);
    let mut configured = snapshot.clone();
    configured.sources.insert(
        "models/main.mod".to_owned(),
        SourceFact::Text {
            text: "@#include \"a.data\"\n".to_owned(),
        },
    );
    configured.search_paths = vec!["SHARED".to_owned()];
    match capture_git_snapshot(&configured) {
        SnapshotCapture::Failure(failure) => assert_eq!(failure.code, "INCOMPLETE_INPUT"),
        _ => panic!("tree lookup must preserve case and reject unrelated suffix matches"),
    }
}

#[test]
fn missing_historical_source_cannot_use_a_current_disk_file() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let filename = format!(
        "snapshot-current-only-{}-{}.inc",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    std::fs::write(&filename, "var from_disk; model; from_disk=1; end;").unwrap();
    let root = format!("@#include \"{filename}\"\n");
    let snapshot = input("before", "root.mod", &[("root.mod", &root)]);
    let captured = capture_git_snapshot(&snapshot);
    std::fs::remove_file(&filename).unwrap();
    match captured {
        SnapshotCapture::Failure(failure) => {
            assert_eq!(failure.code, "INCOMPLETE_INPUT");
            assert_eq!(failure.file_key.as_deref(), Some(filename.as_str()));
        }
        _ => panic!("disk must not fill a historical gap"),
    }
}

#[test]
fn unsupported_history_dependencies_and_read_failures_remain_explicit() {
    for (target, key, mode) in [
        ("link", "link", "120000"),
        ("module/part", "module", "160000"),
    ] {
        let root = format!("@#include \"{target}\"\n");
        let mut snapshot = input(
            "before",
            "root.mod",
            &[("root.mod", &root), (key, "ignored")],
        );
        snapshot.manifest.get_mut(key).unwrap().mode = mode.to_owned();
        match capture_git_snapshot(&snapshot) {
            SnapshotCapture::Failure(failure) => {
                assert_eq!(failure.code, "UNSUPPORTED_SOURCE");
                assert_eq!(failure.file_key.as_deref(), Some(target));
            }
            _ => panic!("must refuse {mode}"),
        }
    }
    let outside = input(
        "before",
        "root.mod",
        &[("root.mod", "@#include \"../other.inc\"\n")],
    );
    assert!(
        matches!(capture_git_snapshot(&outside), SnapshotCapture::Failure(failure) if failure.code == "UNSUPPORTED_SOURCE")
    );
    let mut read_error = input(
        "before",
        "root.mod",
        &[("root.mod", "@#include \"part\"\n"), ("part", "ignored")],
    );
    read_error.sources.insert(
        "part".to_owned(),
        SourceFact::Failure {
            code: "missing_object".to_owned(),
            message: "Blob is not on this machine".to_owned(),
        },
    );
    assert!(
        matches!(capture_git_snapshot(&read_error), SnapshotCapture::Failure(failure) if failure.code == "missing_object" && failure.file_key.as_deref() == Some("part"))
    );
}

#[test]
fn same_path_at_two_commits_has_two_source_identities_and_exact_utf16_ranges() {
    let before = input(
        "before",
        "root.mod",
        &[
            ("root.mod", "@#include \"part\"\n"),
            (
                "part",
                "// 🧭\nvar y;\nmodel;\n/* 🧭 */ [name='r'] y=1;\nend;\n",
            ),
        ],
    );
    let after = input(
        "after",
        "root.mod",
        &[
            ("root.mod", "@#include \"part\"\r\n"),
            (
                "part",
                "// 🧭\r\nvar y;\r\nmodel;\r\n/* 🧭 */ [name='r'] y=2;\r\nend;\r\n",
            ),
        ],
    );
    let result =
        compare_captured_snapshots(ready(&before), ready(&after), SnapshotCoordinates::Lsp);
    assert_eq!(result["state"], "result");
    assert_eq!(result["navigation"]["schema_version"], 2);
    let row = changed_row(&result);
    for side in ["before", "after"] {
        let location = &row[side]["written_locations"][0];
        assert_eq!(location["input_id"], side);
        assert_eq!(location["file_key"], "part");
        assert_eq!(
            location["commit"],
            if side == "before" {
                "a".repeat(40)
            } else {
                "b".repeat(40)
            }
        );
        assert!(location.get("uri").is_none());
        assert_eq!(location["range"]["start"]["line"], 3);
        assert_eq!(location["range"]["start"]["character"], 9);
    }
    let mcp = compare_captured_snapshots(ready(&before), ready(&after), SnapshotCoordinates::Mcp);
    assert_eq!(
        changed_row(&mcp)["after"]["written_locations"][0]["line"],
        4
    );
    assert_eq!(
        changed_row(&mcp)["after"]["written_locations"][0]["column"],
        9
    );
    let mut equivalent = after.clone();
    if let Some(SourceFact::Text { text }) = equivalent.sources.get_mut("part") {
        *text = text.replace("y=2", "y=1");
    }
    let same =
        compare_captured_snapshots(ready(&before), ready(&equivalent), SnapshotCoordinates::Lsp);
    assert_eq!(same["diff"]["changed_equations"], serde_json::json!([]));
    assert_eq!(same["diff"]["added_equations"], serde_json::json!([]));
}

#[test]
fn working_and_historical_resolvers_agree_for_supported_candidates() {
    let directory =
        std::env::temp_dir().join(format!("dygnosis-snapshot-resolver-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("models")).unwrap();
    std::fs::create_dir_all(directory.join("shared")).unwrap();
    let files = [
        (
            "models/root.mod",
            "@#includepath \"../shared\"\n@#include \"body.data\"\n",
        ),
        ("shared/body.data", "var y; model; [name='r'] y=1; end;"),
    ];
    for (key, text) in files {
        std::fs::write(directory.join(key), text).unwrap();
    }
    let historical = ready(&input("before", "models/root.mod", &files));
    let working = match capture_working_snapshot(
        Workspace::new(),
        directory.join("models/root.mod").to_str().unwrap(),
        "after",
        "saved_files",
    ) {
        SnapshotCapture::Ready(captured) => *captured,
        _ => panic!("working capture failed"),
    };
    let result = compare_captured_snapshots(historical, working, SnapshotCoordinates::Lsp);
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(result["state"], "result", "{result}");
    for key in [
        "changed_equations",
        "added_equations",
        "removed_equations",
        "added_endogenous",
        "removed_endogenous",
    ] {
        assert_eq!(result["diff"][key], serde_json::json!([]), "{key}");
    }
    assert_eq!(result["inputs"]["after"]["source_policy"], "saved_files");
    assert!(
        result["inputs"]["after"]["dependency_candidates"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
}

#[test]
fn failed_complete_input_has_no_change_arrays_and_root_absence_is_not_empty_text() {
    let mut absent = input("before", "root.mod", &[]);
    assert!(
        matches!(capture_git_snapshot(&absent), SnapshotCapture::Failure(failure) if failure.code == "ROOT_NOT_FOUND")
    );
    absent.sources.insert(
        "root.mod".to_owned(),
        SourceFact::Failure {
            code: "missing_source".to_owned(),
            message: "Root is absent from the selected tree".to_owned(),
        },
    );
    assert!(
        matches!(capture_git_snapshot(&absent), SnapshotCapture::Failure(failure) if failure.code == "ROOT_NOT_FOUND")
    );
    absent.sources.clear();
    absent.manifest = BTreeMap::from([(
        "root.mod".to_owned(),
        ManifestEntry {
            mode: "100644".to_owned(),
            object_id: "c".repeat(40),
        },
    )]);
    assert!(
        matches!(capture_git_snapshot(&absent), SnapshotCapture::NeedsSources(keys) if keys == ["root.mod"])
    );
}

#[test]
fn external_candidate_before_tree_match_cannot_be_assumed_absent() {
    let mut snapshot = input(
        "before",
        "root.mod",
        &[
            ("root.mod", "@#include \"part\"\n"),
            ("shared/part", "var y; model; [name='r'] y=1; end;"),
        ],
    );
    snapshot.search_paths = vec!["/outside".to_owned(), "shared".to_owned()];
    assert!(
        matches!(capture_git_snapshot(&snapshot), SnapshotCapture::Failure(failure) if failure.code == "UNSUPPORTED_SOURCE")
    );
    snapshot.search_paths.reverse();
    assert!(matches!(
        capture_git_snapshot(&snapshot),
        SnapshotCapture::Ready(_)
    ));
    snapshot.search_paths.reverse();
    snapshot
        .manifest
        .insert("part".to_owned(), snapshot.manifest["shared/part"].clone());
    snapshot
        .sources
        .insert("part".to_owned(), snapshot.sources["shared/part"].clone());
    assert!(matches!(
        capture_git_snapshot(&snapshot),
        SnapshotCapture::Ready(_)
    ));
}

#[test]
fn saved_working_capture_rejects_source_changes_and_created_negative_candidates() {
    let directory =
        std::env::temp_dir().join(format!("dygnosis-snapshot-changing-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("common")).unwrap();
    let root = directory.join("root.mod");
    let body = directory.join("common/body");
    std::fs::write(&root, "@#include \"body\"\n").unwrap();
    std::fs::write(&body, "var y; model; [name='r'] y=1; end;").unwrap();
    let capture = |id| {
        let workspace = Workspace::with_search_paths(vec![directory.join("common")]);
        match capture_working_snapshot(workspace, root.to_str().unwrap(), id, "saved_files") {
            SnapshotCapture::Ready(input) => *input,
            _ => panic!("Working capture failed"),
        }
    };
    let historical = || {
        ready(&input(
            "before",
            "root.mod",
            &[("root.mod", "var y; model; [name='r'] y=1; end;")],
        ))
    };
    let captured = capture("after");
    std::fs::write(&body, "var y; model; [name='r'] y=2; end;").unwrap();
    let changed = compare_captured_snapshots(historical(), captured, SnapshotCoordinates::Lsp);
    assert_eq!(changed["code"], "INPUT_CHANGED");
    assert!(changed.get("diff").is_none());
    let captured = capture("after");
    std::fs::write(directory.join("body"), "var y; model; [name='r'] y=3; end;").unwrap();
    let changed = compare_captured_snapshots(historical(), captured, SnapshotCoordinates::Lsp);
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(changed["code"], "INPUT_CHANGED");
    assert_eq!(changed["side"], "after");
    assert!(changed.get("navigation").is_none());
}

#[test]
fn absolute_paths_inside_repository_use_the_selected_tree_and_outside_paths_refuse() {
    let mut snapshot = input(
        "before",
        "models/root.mod",
        &[
            (
                "models/root.mod",
                "@#include \"C:/snapshot-test/shared/body\"\n",
            ),
            ("shared/body", "var y; model; [name='r'] y=1; end;"),
        ],
    );
    snapshot.repository_uri = "file:///C:/snapshot-test".to_owned();
    assert!(ready(&snapshot).sources.contains_key("shared/body"));
    snapshot.sources.insert(
        "models/root.mod".to_owned(),
        SourceFact::Text {
            text: "@#includepath \"C:/snapshot-test/shared\"\n@#include \"body\"\n".to_owned(),
        },
    );
    assert!(ready(&snapshot).sources.contains_key("shared/body"));
    snapshot.sources.insert(
        "models/root.mod".to_owned(),
        SourceFact::Text {
            text: "@#include \"C:/another-repository/shared/body\"\n".to_owned(),
        },
    );
    assert!(
        matches!(capture_git_snapshot(&snapshot), SnapshotCapture::Failure(failure) if failure.code == "UNSUPPORTED_SOURCE")
    );
}

#[test]
fn git_source_pairing_requires_the_exact_repository_and_keeps_empty_includes() {
    let root = "@#include \"empty\"\n@#include \"body\"\n";
    let before = input(
        "before",
        "root.mod",
        &[("root.mod", root), ("empty", ""), ("body", "% old\n")],
    );
    let mut after = input(
        "after",
        "root.mod",
        &[("root.mod", root), ("empty", ""), ("body", "% new\n")],
    );
    let paired =
        compare_captured_snapshots(ready(&before), ready(&after), SnapshotCoordinates::Lsp);
    assert_eq!(paired["sources"]["before"]["empty"], "");
    assert_eq!(
        paired["diff"]["source_changes"]["files"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        paired["diff"]["source_changes"]["files"][0]["correspondence"],
        "proven_file_identity"
    );
    assert_eq!(
        paired["diff"]["source_changes"]["files"][0]["before"]["input_id"],
        "before"
    );
    after.repository_uri = "file:///different-snapshot-repository".to_owned();
    let unpaired =
        compare_captured_snapshots(ready(&before), ready(&after), SnapshotCoordinates::Lsp);
    let files = unpaired["diff"]["source_changes"]["files"]
        .as_array()
        .unwrap();
    assert_eq!(files.len(), 4, "{unpaired}");
    assert!(files
        .iter()
        .all(|file| file["correspondence"] == "unpaired"));
}

#[test]
fn git_working_source_aliases_pair_repository_relative_includes_lexically() {
    let directory = std::env::temp_dir().join(format!("dygnosis-aliases-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("shared")).unwrap();
    let root = "@#include \"shared/./part\"\n@#include \"shared/empty\"\n";
    std::fs::write(directory.join("root.mod"), root).unwrap();
    std::fs::write(directory.join("shared/part"), "% new\n").unwrap();
    std::fs::write(directory.join("shared/empty"), "").unwrap();
    let mut before = input(
        "before",
        "root.mod",
        &[
            ("root.mod", root),
            ("shared/part", "% old\n"),
            ("shared/empty", ""),
        ],
    );
    before.repository_uri = tower_lsp::lsp_types::Url::from_file_path(&directory)
        .unwrap()
        .to_string();
    let working = match capture_working_snapshot(
        Workspace::new(),
        directory.join("root.mod").to_str().unwrap(),
        "after",
        "saved_files",
    ) {
        SnapshotCapture::Ready(captured) => *captured,
        _ => panic!("working capture"),
    };
    let result = compare_captured_snapshots(ready(&before), working, SnapshotCoordinates::Mcp);
    std::fs::remove_dir_all(directory).unwrap();
    let files = result["diff"]["source_changes"]["files"]
        .as_array()
        .unwrap();
    assert_eq!(files.len(), 1, "{result}");
    assert_eq!(files[0]["correspondence"], "proven_file_identity");
    assert_eq!(files[0]["before"]["file_key"], "shared/part");
    assert!(files[0]["after"]["file_key"]
        .as_str()
        .unwrap()
        .ends_with(if cfg!(windows) {
            "shared\\part"
        } else {
            "shared/part"
        }));
}
