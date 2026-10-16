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

// Every accepted row slot and value must participate in the shared binding;
// otherwise navigation loses uses and complete Rename is refused.
#[test]
fn syntax_rows_retain_all_ordinary_binding_sites() {
    for tail in [
        "priors;a,0.5,,,beta_pdf,0.5,0.1;end;",
        "conditional_forecast_paths;var y;periods 1;values (a);end;",
        "moment_calibration;y,y,[-1,1];end;",
        "irf_calibration;y,e,[0,1];end;",
        "varobs y;observation_trends;y(a);end;",
        "deterministic_trends;y(a);end;",
    ] {
        let text = format!("var y;varexo e;parameters a;a=0.5;model;y=a*y(-1)+e;end;{tail}");
        let view = model(&text);
        for name in ["a", "y", "e"] {
            let binding = choose(&view, name);
            for site in view.references(&binding, true) {
                if let Some((file, span)) = site.written {
                    assert_eq!(
                        view.select(Selector::Target {
                            file: &file,
                            byte: span.start
                        })
                        .unwrap()
                        .unwrap()
                        .id,
                        binding.id
                    );
                }
            }
            assert!(
                rename(
                    std::slice::from_ref(&view),
                    0,
                    &binding,
                    &format!("renamed_{name}")
                )
                .is_ok(),
                "{tail}: {name}"
            );
        }
    }
}

// Special operators keep their exact keyword range and do not become an
// ordinary symbol, while their argument retains its own binding.
#[test]
fn special_operators_have_queryable_calls_and_no_editable_declaration() {
    let text = "var y;model;y=EXPECTATION(-1)(y)+steady_state(y);end;";
    let view = model(text);
    for (name, spelling) in [
        ("expectation", "EXPECTATION"),
        ("STEADY_STATE", "steady_state"),
    ] {
        let binding = choose(&view, name);
        assert_eq!(binding.kind, "builtin_function");
        assert!(binding.declaration.is_none());
        let sites = view.references(&binding, true);
        assert_eq!(sites.len(), 1);
        let (_, span) = sites[0].written.as_ref().unwrap();
        assert_eq!(&text[span.start as usize..span.end as usize], spelling);
        assert_eq!(
            view.select(Selector::Target {
                file: "root.mod",
                byte: span.start
            })
            .unwrap()
            .unwrap()
            .id,
            binding.id
        );
        assert_eq!(
            rename(std::slice::from_ref(&view), 0, &binding, "other")
                .unwrap_err()
                .code,
            "BUILTIN_SYMBOL"
        );
    }
}

// A recovered controlled block must not claim the complete source needed for
// Rename, even if all ordinary declarations and equations remain available.
#[test]
fn refused_controlled_path_withholds_complete_source_claim() {
    let view = model(
        "var y;varexo e;model;y=e;end;perfect_foresight_controlled_paths;exogenize y;endogenize e;periods 1;values 2;end;",
    );
    assert!(!view.model.controlled_paths[0].completed);
    assert!(!view.complete);
    assert_eq!(
        rename(std::slice::from_ref(&view), 0, &choose(&view, "y"), "z").unwrap_err(),
        INCOMPLETE
    );
}

// Family-owned E001 syntax records must withhold the same complete source
// claim as parser-owned errors. Separator recovery cannot make Rename safe.
#[test]
fn refused_family_syntax_withholds_complete_source_claim() {
    for tail in [
        "moment_calibration;y,y,-1,1;end;",
        "observation_trends;y(a+);end;",
    ] {
        let view = model(&format!("var y;parameters a;model;y=a;end;{tail}"));
        assert!(!view.complete, "{tail}");
        assert_eq!(
            rename(std::slice::from_ref(&view), 0, &choose(&view, "y"), "z").unwrap_err(),
            INCOMPLETE
        );
    }
}

// These ordinary list readers are shared by conditional and controlled paths.
// A completed separator cannot hide refused tokens or run stanza callbacks.
#[test]
fn refused_ordinary_path_lists_withhold_completion_and_callbacks() {
    for (periods, values) in [
        ("1", "-2^2"),
        ("1,", "2"),
        ("1 garbage", "2"),
        (":1", "2"),
        ("1:", "2"),
    ] {
        for tail in [
            format!(
                "perfect_foresight_controlled_paths;exogenize y;periods {periods};values {values};endogenize e;end;"
            ),
            format!("conditional_forecast_paths;var y;periods {periods};values {values};end;"),
        ] {
            let view = model(&format!("var y;varexo e;model;y=e;end;{tail}"));
            assert!(!view.complete, "{tail}");
            assert!(
                view.model
                    .controlled_paths
                    .iter()
                    .all(|block| !block.completed
                        && block.stanzas.iter().all(|row| !row.callback_completed)),
                "{tail}"
            );
            assert_eq!(
                rename(std::slice::from_ref(&view), 0, &choose(&view, "y"), "z").unwrap_err(),
                INCOMPLETE
            );
        }
    }
}

// DATE is grammatical in a period list, but the conditional block callback
// refuses it and therefore cannot support a complete source edit proposal.
#[test]
fn conditional_date_callback_withholds_complete_source_claim() {
    let view =
        model("var y;model;y=0;end;conditional_forecast_paths;var y;periods 2000Q1;values 2;end;");
    assert!(!view.complete);
    assert_eq!(
        rename(std::slice::from_ref(&view), 0, &choose(&view, "y"), "z").unwrap_err(),
        INCOMPLETE
    );
}

// A worker stop during semantic occurrence or Rename verification must discard
// its unfinished result; pure calls outside that worker scope still complete.
#[test]
fn cancellation_discards_symbol_reference_and_rename_work() {
    use std::sync::{Arc, atomic::AtomicBool};
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
    assert!(
        !rename(&[view], 0, &binding, "rho")
            .unwrap()
            .edits
            .is_empty()
    );
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
    assert!(
        written
            .iter()
            .any(|span| span.start as usize == text.find("order;").unwrap())
    );
    assert!(
        written
            .iter()
            .any(|span| span.start as usize == text.find("order=0.9").unwrap())
    );
    assert!(
        written
            .iter()
            .any(|span| span.start as usize == text.find("y=order").unwrap() + 2)
    );
    assert!(
        view.select(Selector::Target {
            file: "root.mod",
            byte: text.find("order=1").unwrap() as u32
        })
        .unwrap()
        .is_none()
    );
    assert_eq!(rename(&[view], 0, &binding, "rho").unwrap_err(), BINDING);
}

// A name selects scoped candidates; a written use selects its own definition.
#[test]
fn local_candidates_and_selected_scope_are_distinct() {
    let text = "heterogeneity_dimension h k;\nvar(heterogeneity=h) a;var(heterogeneity=k) b;\nmodel(heterogeneity=h);#helper=1;a=helper;end;\nmodel(heterogeneity=k);#helper=2;b=helper;end;";
    let view = model(text);
    let ambiguous = view.select(Selector::Name("helper")).unwrap_err();
    assert_eq!(ambiguous.reason.code, "AMBIGUOUS_SYMBOL");
    assert!(
        ambiguous
            .candidates
            .iter()
            .any(|binding| binding.dimension.as_deref() == Some("h"))
    );
    assert!(
        ambiguous
            .candidates
            .iter()
            .any(|binding| binding.dimension.as_deref() == Some("k"))
    );
    let byte = text.find("b=helper").unwrap() + 2;
    let binding = view
        .select(Selector::Target {
            file: "root.mod",
            byte: byte as u32,
        })
        .unwrap()
        .unwrap();
    assert_eq!(binding.dimension.as_deref(), Some("k"));
    assert!(
        view.references(&binding, true)
            .iter()
            .all(|site| site.dimension.as_deref() == Some("k"))
    );
    let proposal = rename(&[view], 0, &binding, "firm_helper").unwrap();
    assert!(
        proposal
            .edits
            .iter()
            .all(|edit| edit.span.start as usize >= text.find("model(heterogeneity=k)").unwrap())
    );
}

// Selecting one shared declaration does not waive the other scopes' uses.
#[test]
fn shared_local_declaration_refuses_complete_rename() {
    let view = model(
        "model_local_variable helper;heterogeneity_dimension h k;var(heterogeneity=h) a;var(heterogeneity=k) b;model(heterogeneity=h);#helper=1;a=helper;end;model(heterogeneity=k);#helper=2;b=helper;end;",
    );
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
    assert!(view.references(&binding, true).iter().any(|site| {
        site.role == "tag_reference"
            && site
                .written
                .as_ref()
                .is_some_and(|(_, span)| span.start == byte)
    }));
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
    assert!(
        proposal
            .edits
            .iter()
            .all(|edit| edit.file == "decl.inc" || edit.file == "equation.inc")
    );

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
    assert!(
        view.model
            .statements
            .iter()
            .any(|statement| statement.name == "verbatim")
    );
    assert!(
        view.references(&binding, true)
            .iter()
            .all(|site| !native_orders.contains(&site.order))
    );
    assert_eq!(rename(&[view], 0, &binding, "rho").unwrap_err(), BINDING);
}

// Parse refusal takes precedence over a later binding-capture decision.
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
            "INCOMPLETE_INPUT"
        );
    }
    // VAREXP's expression accepts an unresolved name at Parse. Rename still
    // must not give that existing read the parameter's binding.
    let view = model(
        "parameters helper;var_expectation_model(model_name=m,expression=helper+other,auxiliary_model_name=aux,horizon=1);",
    );
    let binding = choose(&view, "helper");
    assert_eq!(
        rename(&[view], 0, &binding, "other").unwrap_err().code,
        "BINDING_CAPTURE"
    );
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

// Guards bulk color-site projection using equal text as evidence. Only the
// parser's declaration/write/read sites can acquire an ordinary role.
#[test]
fn accepted_ordinary_inventory_excludes_native_and_named_lookalikes() {
    let text = "parameters a;a=.5;var y;model;[name='Y']y=a*y(-1);end;var_model(model_name=a,eqtags=['Y']);verbatim;a = a + y;end;";
    let view = model(text);
    let sites: Vec<_> = view
        .accepted_ordinary_sites()
        .filter(|(name, _)| *name == "a")
        .collect();
    let ordinary = view
        .bindings()
        .into_iter()
        .find(|row| row.name == "a" && row.is_ordinary())
        .unwrap();
    let references = view.references(&ordinary, true);
    assert_eq!(sites.len(), 3);
    assert_eq!(
        sites.iter().map(|(_, site)| site.order).collect::<Vec<_>>(),
        references.iter().map(|site| site.order).collect::<Vec<_>>()
    );
    let named_start = text.find("model_name=a").unwrap() as u32 + "model_name=".len() as u32;
    let native_start = text.find("verbatim;").unwrap() as u32;
    assert!(
        sites
            .iter()
            .all(|(_, site)| site.span.start != named_start && site.span.start < native_start)
    );
    let without_declaration = view.references(&ordinary, false);
    assert_eq!(without_declaration.len(), 2);
    assert!(without_declaration.iter().all(|site| !site.declaration));
}
