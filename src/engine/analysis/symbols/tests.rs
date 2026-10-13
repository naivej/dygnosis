//! Shared binding tests guard semantic selection before transport conversion.

use super::*;
use std::collections::BTreeMap;

fn model(text: &str) -> SemanticModel {
    capture("root.mod", &[("root.mod", text)], "revision")
}

fn capture(root: &str, files: &[(&str, &str)], revision: &str) -> SemanticModel {
    let files: BTreeMap<_, _> = files
        .iter()
        .map(|(file, text)| ((*file).into(), (*text).into()))
        .collect();
    let mut workspace = Workspace::overlay_documents(&files);
    SemanticModel::capture(&mut workspace, root, revision.into()).unwrap()
}

fn choose(view: &SemanticModel, name: &str) -> Binding {
    view.select(Selector::Name(name)).unwrap().unwrap()
}

// A worker stop during semantic occurrence or Rename verification must discard
// its unfinished result; pure calls outside that worker scope still complete.
#[test]
fn cancellation_discards_symbol_reference_and_rename_work() {
    use std::sync::{atomic::AtomicBool, Arc};
    let view = model("var y;parameters p;model;\n@#for i in 1:100\ny=p;\n@#endfor\nend;");
    let binding = choose(&view, "p");
    let flag = Arc::new(AtomicBool::new(false));
    crate::engine::control::cancel_after(&flag, 8);
    let result = crate::engine::control::run(Some(flag), || view.references(&binding, true));
    assert!(
        result.is_err(),
        "cancelled occurrence walk must not publish rows"
    );
    let flag = Arc::new(AtomicBool::new(false));
    crate::engine::control::cancel_after(&flag, 8);
    let result = crate::engine::control::run(Some(flag), || {
        rename(std::slice::from_ref(&view), 0, &binding, "rho")
    });
    assert!(
        result.is_err(),
        "cancelled verification must not publish edits"
    );
    assert!(!rename(&[view], 0, &binding, "rho")
        .unwrap()
        .edits
        .is_empty());
}

// A filename, option key, native line or comment cannot become a bound use.
#[test]
fn ordinary_references_use_accepted_name_slots() {
    let text = "var y;parameters order;order=0.9;model;y=order;end;\nstoch_simul(order=1) y;\nload_params_and_steady_state(order);\nverbatim;\norder=2;\nend;\n// order\n";
    let view = model(text);
    let binding = choose(&view, "order");
    let references = view.references(&binding, true);
    let written: Vec<_> = references
        .iter()
        .filter_map(|site| site.written.as_ref())
        .map(|(_, span)| *span)
        .collect();
    for marker in ["order=1", "order);", "order=2", "// order"] {
        let start = text.find(marker).unwrap() + usize::from(marker.starts_with("// ")) * 3;
        assert!(
            !written.iter().any(|span| span.start == start as u32),
            "unbound slot: {marker}"
        );
    }
    assert!(written
        .iter()
        .any(|span| span.start as usize == text.find("order;").unwrap()));
    assert!(written
        .iter()
        .any(|span| span.start as usize == text.find("order=0.9").unwrap()));
    assert!(written
        .iter()
        .any(|span| span.start as usize == text.find("y=order").unwrap() + 2));
    assert!(view
        .select(Selector::Target {
            file: "root.mod",
            byte: text.find("order=1").unwrap() as u32
        })
        .unwrap()
        .is_none());
    assert_eq!(rename(&[view], 0, &binding, "rho").unwrap_err(), BINDING);
}

// A name selects scoped candidates; a written use selects its own definition.
#[test]
fn local_candidates_and_selected_scope_are_distinct() {
    let text = "heterogeneity_dimension h k;\nvar(heterogeneity=h) a;var(heterogeneity=k) b;\nmodel(heterogeneity=h);#helper=1;a=helper;end;\nmodel(heterogeneity=k);#helper=2;b=helper;end;";
    let view = model(text);
    let ambiguous = view.select(Selector::Name("helper")).unwrap_err();
    assert_eq!(ambiguous.reason.code, "AMBIGUOUS_SYMBOL");
    assert!(ambiguous
        .candidates
        .iter()
        .any(|binding| binding.dimension.as_deref() == Some("h")));
    assert!(ambiguous
        .candidates
        .iter()
        .any(|binding| binding.dimension.as_deref() == Some("k")));
    let byte = text.find("b=helper").unwrap() + 2;
    let binding = view
        .select(Selector::Target {
            file: "root.mod",
            byte: byte as u32,
        })
        .unwrap()
        .unwrap();
    assert_eq!(binding.dimension.as_deref(), Some("k"));
    assert!(view
        .references(&binding, true)
        .iter()
        .all(|site| site.dimension.as_deref() == Some("k")));
    let proposal = rename(&[view], 0, &binding, "firm_helper").unwrap();
    assert!(proposal
        .edits
        .iter()
        .all(|edit| edit.span.start as usize >= text.find("model(heterogeneity=k)").unwrap()));
}

// Selecting one shared declaration does not waive the other scopes' uses.
#[test]
fn shared_local_declaration_refuses_complete_rename() {
    let view = model("model_local_variable helper;heterogeneity_dimension h k;var(heterogeneity=h) a;var(heterogeneity=k) b;model(heterogeneity=h);#helper=1;a=helper;end;model(heterogeneity=k);#helper=2;b=helper;end;");
    let candidates = view
        .select(Selector::Name("helper"))
        .unwrap_err()
        .candidates;
    assert_eq!(
        rename(&[view], 0, &candidates[0], "next_helper").unwrap_err(),
        BINDING
    );
}

// Repeated macro executions retain occurrences while one source edit is unique.
#[test]
fn repeated_occurrences_keep_identity_and_deduplicate_edits() {
    let text = "var y;parameters p;model;\n@#for i in [1,2]\ny=p;\n@#endfor\nend;";
    let view = model(text);
    let binding = choose(&view, "p");
    let occurrences = view.references(&binding, false);
    let uses: Vec<_> = occurrences
        .iter()
        .filter(|site| site.role == "use")
        .collect();
    assert_eq!(uses.len(), 2);
    assert_ne!(uses[0].order, uses[1].order);
    assert_eq!(uses[0].written, uses[1].written);
    let proposal = rename(&[view], 0, &binding, "rho").unwrap();
    assert_eq!(
        proposal
            .edits
            .iter()
            .filter(|edit| edit.span.start as usize == text.find("y=p").unwrap() + 2)
            .count(),
        1
    );
}

// Literal semantic names can be edited without changing quote or bound text.
#[test]
fn tag_references_and_generated_bounds_preserve_literal_edits() {
    let text = "@#define bound=0\nvar y;model;[name='label y',mcp='  y  > @{bound}'] y=0;end;";
    let view = model(text);
    let byte = text.find("y  >").unwrap() as u32;
    let binding = view
        .select(Selector::Target {
            file: "root.mod",
            byte,
        })
        .unwrap()
        .unwrap();
    assert!(view
        .references(&binding, true)
        .iter()
        .any(|site| site.role == "tag_reference"
            && site
                .written
                .as_ref()
                .is_some_and(|(_, span)| span.start == byte)));
    let proposal = rename(&[view], 0, &binding, "z").unwrap();
    let mut result = text.to_owned();
    for edit in proposal.edits.iter().rev() {
        result.replace_range(
            edit.span.start as usize..edit.span.end as usize,
            &edit.replacement,
        );
    }
    assert!(
        result.contains("[name='label y',mcp='  z  > @{bound}'] z=0;"),
        "{result}"
    );
}

// A stale binding ID cannot silently select the same name in a new snapshot.
#[test]
fn binding_identity_requires_its_original_snapshot() {
    let first = capture("root.mod", &[("root.mod", "var y;model;y=1;end;")], "first");
    let binding = choose(&first, "y");
    let second = capture(
        "root.mod",
        &[("root.mod", "var y;model;y=2;end;")],
        "second",
    );
    assert!(second.select(Selector::Id(&binding.id)).unwrap().is_none());
    assert_eq!(
        rename(&[second], 0, &binding, "z").unwrap_err().code,
        "INPUT_CHANGED"
    );
}

// Built-in calls have accepted references but never ordinary Rename ownership.
#[test]
fn builtin_calls_are_explicit_and_do_not_select_native_lookalikes() {
    let text = "var y;model;y=exp(y(-1));end;\nverbatim;\nexp(y);\nend;";
    let view = model(text);
    let binding = choose(&view, "exp");
    assert_eq!(binding.kind, "builtin_function");
    let sites = view.references(&binding, true);
    assert_eq!(sites.len(), 1);
    assert_eq!(sites[0].role, "call");
    assert_eq!(
        sites[0].written.as_ref().unwrap().1.start as usize,
        text.find("exp").unwrap()
    );
    assert_eq!(
        rename(&[view], 0, &binding, "other").unwrap_err().code,
        "BUILTIN_SYMBOL"
    );
}

// Shared edits require compatible declarations, and collisions in any owner
// withhold the entire proposal. An unrelated owner's spelling adds no edits.
#[test]
fn rename_closes_over_only_affected_checked_owners() {
    let files = [
        (
            "first.mod",
            "@#include \"decl.inc\"\nvar y;model;\n@#include \"equation.inc\"\nend;",
        ),
        (
            "second.mod",
            "@#include \"decl.inc\"\nvar y;model;\n@#include \"equation.inc\"\nend;",
        ),
        ("unrelated.mod", "parameters p;var y;model;y=p;end;"),
        ("decl.inc", "parameters p;"),
        ("equation.inc", "y=p;"),
    ];
    let first = capture("first.mod", &files, "revision");
    let second = capture("second.mod", &files, "revision");
    let unrelated = capture("unrelated.mod", &files, "revision");
    let binding = choose(&first, "p");
    let proposal = rename(&[first, second, unrelated], 0, &binding, "rho").unwrap();
    assert_eq!(proposal.roots, vec!["first.mod", "second.mod"]);
    assert!(proposal
        .edits
        .iter()
        .all(|edit| edit.file == "decl.inc" || edit.file == "equation.inc"));

    let first = capture("first.mod", &files, "revision");
    let collision_files = [
        files[0],
        (
            "second.mod",
            "@#include \"decl.inc\"\nparameters rho;var y;model;\n@#include \"equation.inc\"\nend;",
        ),
        files[3],
        files[4],
    ];
    let second = capture("second.mod", &collision_files, "revision");
    let binding = choose(&first, "p");
    assert_eq!(
        rename(&[first, second], 0, &binding, "rho").unwrap_err(),
        COLLISION
    );
}

// One macro literal cannot be edited when one execution is native MATLAB.
#[test]
fn repeated_literal_with_native_execution_withholds_rename() {
    let text = "parameters p;var y;\n@#for block in [\"model\",\"verbatim\"]\n@{block};\ny=p;\nend;\n@#endfor\n";
    let view = model(text);
    let binding = choose(&view, "p");
    let native_orders = ordinary::native_orders(&view.model);
    assert!(view
        .model
        .statements
        .iter()
        .any(|statement| statement.name == "verbatim"));
    assert!(view
        .references(&binding, true)
        .iter()
        .all(|site| !native_orders.contains(&site.order)));
    assert_eq!(rename(&[view], 0, &binding, "rho").unwrap_err(), BINDING);
}

// Rename cannot give a previously unresolved read a different binding.
#[test]
fn rename_refuses_to_capture_unresolved_replacement_uses() {
    for text in [
        "var y;model;#helper=1;y=helper+other;end;",
        "var y;parameters helper;model;y=helper+other;end;",
    ] {
        let view = model(text);
        let binding = choose(&view, "helper");
        assert_eq!(
            rename(&[view], 0, &binding, "other").unwrap_err().code,
            "BINDING_CAPTURE"
        );
    }
}

// Compatible owners can share one verified local declaration and definition.
// Different definitions or scopes must withhold all edits.
#[test]
fn shared_local_owners_require_matching_written_binding_and_scope() {
    let files = [
        (
            "a.mod",
            "var y;\n@#include \"decl.inc\"\nmodel;\n@#include \"def.inc\"\ny=helper;end;",
        ),
        (
            "b.mod",
            "var z;\n@#include \"decl.inc\"\nmodel;\n@#include \"def.inc\"\nz=helper;end;",
        ),
        ("decl.inc", "model_local_variable helper;"),
        ("def.inc", "#helper=1;"),
    ];
    let first = capture("a.mod", &files, "revision");
    let second = capture("b.mod", &files, "revision");
    let binding = choose(&first, "helper");
    let proposal = rename(&[first, second], 0, &binding, "shared_helper").unwrap();
    assert_eq!(proposal.roots, vec!["a.mod", "b.mod"]);
    assert_eq!(
        proposal
            .edits
            .iter()
            .filter(|edit| edit.file == "def.inc")
            .count(),
        1
    );
    assert!(proposal.edits.iter().any(|edit| edit.file == "a.mod"));
    assert!(proposal.edits.iter().any(|edit| edit.file == "b.mod"));

    let first = capture("a.mod", &files, "revision");
    let other_files = [
        files[0],
        (
            "b.mod",
            "var z;\n@#include \"decl.inc\"\nmodel;#helper=2;z=helper;end;",
        ),
        files[2],
        files[3],
    ];
    let second = capture("b.mod", &other_files, "revision");
    let binding = choose(&first, "helper");
    assert_eq!(
        rename(&[first, second], 0, &binding, "shared_helper").unwrap_err(),
        BINDING
    );
}
