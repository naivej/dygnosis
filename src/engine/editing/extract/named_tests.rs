//! Named model extraction regressions exercise retained facts and real fragments.

use super::*;

fn request(source: &str, name: &str) -> ExtractRequest {
    ExtractRequest {
        file_content: source.into(),
        names: vec![name.into()],
        ..Default::default()
    }
}

fn fragment(source: &str, name: &str) -> ExtractResult {
    let result = extract(&request(source, name)).unwrap();
    assert_eq!(result.status, ExtractStatus::Ok, "{}", result.explanation);
    result
}

fn accepted_at_check(source: &str) {
    if let Some(preprocessor) = crate::find_preprocessor(None) {
        let outcome = crate::run_preprocessor(
            source,
            &preprocessor,
            None,
            std::time::Duration::from_secs(30),
            crate::JsonStage::Check,
        );
        assert!(outcome.success, "{source}\n{outcome:?}");
    }
}

const VAR: &str = "var y x unused;\nvarexo e;\nparameters rho betta weight;\nrho=.5;\nbetta=.99;\nweight=.4;\nmodel;\n# scale=weight;\n[name='aux'] x=rho*x(-1)+e;\n[name='wanted'] y=var_expectation(ve);\n[name='unused'] unused=0;\nend;\nvar_model(model_name=vm,eqtags=['aux']);\nvar_expectation_model(model_name=ve,auxiliary_model_name=vm,horizon=1,expression=scale*x,discount=betta);\n";

#[test]
fn var_expectation_closes_auxiliary_equations_and_definition_expressions() {
    // Guards returning an operator without its definition, tagged equations or local/calibration names.
    accepted_at_check(VAR);
    let result = fragment(VAR, "wanted");
    let text = result.fragment.as_deref().unwrap();
    let parsed = parse(text);
    assert!(parsed.semi_structural_commands.iter().any(
        |command| command.kind == crate::engine::language::model::SemiStructuralKind::VarModel
    ));
    assert!(
        parsed
            .semi_structural_commands
            .iter()
            .any(|command| command.kind
                == crate::engine::language::model::SemiStructuralKind::VarExpectationModel)
    );
    let local = parsed.equations.iter().find(|equation| equation.is_local && equation.lhs_expr.is_some_and(|id| matches!(parsed.exprs.get(id).kind, ExprKind::Ident { name, .. } if parsed.name(name) == "scale"))).expect("scale definition");
    assert!(
        parsed
            .parameters
            .iter()
            .any(|declaration| parsed.name(declaration.name) == "weight"),
        "{text}"
    );
    assert!(
        parsed
            .parameters
            .iter()
            .any(|declaration| parsed.name(declaration.name) == "betta"),
        "{text}"
    );
    assert!(
        !parsed
            .endogenous
            .iter()
            .any(|declaration| parsed.name(declaration.name) == "unused"),
        "{text}"
    );
    assert!(
        result
            .selected_equations
            .iter()
            .any(|row| row.name.as_deref() == Some("aux") && row.role == EquationRole::Companion)
    );
    assert!(result
        .selected_equations
        .iter()
        .any(|row| row.name.as_deref() == Some("wanted") && row.role == EquationRole::Requested));
    assert!(
        parsed
            .semi_structural_commands
            .iter()
            .any(|command| command.kind
                == crate::engine::language::model::SemiStructuralKind::VarExpectationModel
                && local.parse_order < command.parse_order),
        "{text}"
    );
    accepted_at_check(text);
}

const PAC: &str = "var z x v unused;\nvarexo ex ez;\nparameters a b;\na=.5;\nb=.99;\nvar_model(model_name=vm,eqtags=['aux']);\npac_model(model_name=pm,discount=b,auxiliary_model_name=vm);\npac_target_info(pm);\ntarget v;\nauxname_target_nonstationary vns;\ncomponent x;\nauxname px;\nkind dd;\ngrowth 1;\nend;\nmodel;\n[name='aux'] x=x(-1)+ex;\n[name='target'] v=x;\n[name='wanted'] diff(z)=a*(pac_target_nonstationary(pm)-z(-1))+pac_expectation(pm)+ez;\n[name='unused'] unused=0;\nend;\n";

#[test]
fn pac_keeps_composite_target_block_target_equation_and_auxiliary_model() {
    // Guards retaining only the expectation call while dropping the composite target definition.
    accepted_at_check(PAC);
    let result = fragment(PAC, "wanted");
    let text = result.fragment.as_deref().unwrap();
    let parsed = parse(text);
    assert!(
        parsed
            .pac_target_info
            .iter()
            .any(|block| parsed.name(block.name) == "pm")
    );
    assert!(
        parsed
            .equations
            .iter()
            .any(|equation| equation.name == "target")
    );
    assert!(
        parsed
            .equations
            .iter()
            .any(|equation| equation.name == "aux")
    );
    assert!(!text.contains("unused"), "{text}");
    assert!(text.contains("auxname_target_nonstationary vns"), "{text}");
    assert!(text.contains("component x"), "{text}");
    accepted_at_check(text);
}

#[test]
fn transformed_target_matching_keeps_a_proven_aggregate_superset() {
    // Guards refusing a preservable target just because minimal written-LHS matching is unavailable.
    let source = PAC
        .replace("target v;", "target diff(v);")
        .replace("[name='target'] v=x;", "[name='target'] diff(v)=diff(x);");
    let result = fragment(&source, "wanted");
    let text = result.fragment.as_deref().unwrap();
    assert!(
        parse(text)
            .equations
            .iter()
            .any(|equation| equation.name == "unused"),
        "{text}"
    );
    assert!(result.omitted_context.iter().any(
        |note| note.kind == "retained_context" && note.detail.contains("composite PAC target")
    ));
    accepted_at_check(text);
}

#[test]
fn included_macro_copies_keep_selected_named_context_and_unsaved_root() {
    // Guards selecting a similarly spelled definition from another loop iteration or stale supplied root.
    let source = "parameters rho;\nrho=.5;\nvarexo e;\n@#include \"models.inc\"\n";
    let child = "@#for j in 1:2\nvar x@{j} y@{j};\nmodel;\n[name='aux@{j}'] x@{j}=rho*x@{j}(-1)+e;\n[name='wanted@{j}'] y@{j}=var_expectation(ve@{j});\nend;\nvar_model(model_name=vm@{j},eqtags=['aux@{j}']);\nvar_expectation_model(model_name=ve@{j},auxiliary_model_name=vm@{j},horizon=1,expression=x@{j});\n@#endfor\n";
    let mut request = request(source, "wanted2");
    request.active_file = Some("root.mod".into());
    request
        .files
        .insert("root.mod".into(), "@#include \"stale.inc\"\n".into());
    request.files.insert("models.inc".into(), child.into());
    let result = extract(&request).unwrap();
    assert_eq!(result.status, ExtractStatus::Ok, "{}", result.explanation);
    let text = result.fragment.as_deref().unwrap();
    let parsed = parse(text);
    let names: HashSet<_> = parsed
        .semi_structural_commands
        .iter()
        .flat_map(|command| &command.options)
        .filter_map(|option| match &option.value {
            crate::engine::language::model::SemiStructuralValue::Symbol { name, .. }
                if option.name.eq_ignore_ascii_case("model_name") =>
            {
                Some(parsed.name(*name))
            }
            _ => None,
        })
        .collect();
    assert!(
        names.contains("vm2")
            && names.contains("ve2")
            && !names.contains("vm1")
            && !names.contains("ve1"),
        "{text}"
    );
    assert!(
        !parsed.expanded_tokens.iter().any(|token| matches!(
            token.kind,
            crate::engine::language::lexer::TokenKind::MacroDir
                | crate::engine::language::lexer::TokenKind::MacroInterp
        )),
        "{text}"
    );
    assert!(result.origins.iter().any(|origin| {
        origin
            .frames
            .iter()
            .any(|frame| frame.kind == "for" && frame.value.as_deref() == Some("2"))
    }));
    assert!(result.origins.iter().all(|origin| {
        origin
            .file
            .as_deref()
            .is_some_and(|file| file.ends_with("models.inc"))
    }));
    accepted_at_check(text);
}

#[test]
fn a_named_dependency_cycle_terminates_without_duplicate_model_definitions() {
    // Guards a model -> auxiliary equation -> same operator dependency cycle recursing forever or duplicating declarations.
    let source = VAR.replace("x=rho*x(-1)+e", "x=rho*x(-1)+var_expectation(ve)+e");
    let result = fragment(&source, "wanted");
    let text = result.fragment.as_deref().unwrap();
    let parsed = parse(text);
    let names: Vec<_> = parsed
        .semi_structural_commands
        .iter()
        .filter_map(|command| {
            command
                .options
                .iter()
                .find(|option| option.name.eq_ignore_ascii_case("model_name"))
        })
        .filter_map(|option| match option.value {
            crate::engine::language::model::SemiStructuralValue::Symbol { name, .. } => {
                Some(parsed.name(name))
            }
            _ => None,
        })
        .collect();
    assert!(names.contains(&"vm") && names.contains(&"ve"), "{text}");
    assert_eq!(
        names.len(),
        names.iter().copied().collect::<HashSet<_>>().len(),
        "{text}"
    );
    accepted_at_check(text);
}

#[test]
fn missing_or_recovered_named_context_withholds_the_whole_fragment() {
    // Guards success with a missing auxiliary definition or a parser-recovered command.
    for (source, needle) in [
        (
            VAR.replace("auxiliary_model_name=vm", "auxiliary_model_name=missing"),
            "auxiliary model `missing`",
        ),
        (
            VAR.replace(
                "var_model(model_name=vm,eqtags=['aux']);",
                "var_model(model_name=vm,eqtags=['missing']);",
            ),
            "equation tag `missing`",
        ),
        (
            VAR.replace("horizon=1", "horizon=1,not_an_option=1"),
            "not completely accepted",
        ),
        (
            PAC.replace("pac_target_info(pm);\ntarget v;\nauxname_target_nonstationary vns;\ncomponent x;\nauxname px;\nkind dd;\ngrowth 1;\nend;\n", ""),
            "no pac_target_info context",
        ),
        (
            VAR.replace("var_model(model_name=vm,eqtags=['aux']);", "var_model(model_name=vm,eqtags=['aux']);\nvar_model(model_name=vm,eqtags=['aux']);"),
            "more than one definition",
        ),
    ] {
        let result = extract(&request(&source, "wanted")).unwrap();
        assert_eq!(
            result.status,
            ExtractStatus::UnsupportedContext,
            "{result:?}"
        );
        assert!(result.fragment.is_none());
        assert!(
            result.explanation.contains(needle),
            "{}",
            result.explanation
        );
    }
}

#[test]
fn auxiliary_names_follow_each_callers_pinned_namespace_precedence() {
    // Guards taking an equally spelled auxiliary definition from the wrong namespace.
    let extra =
        "trend_component_model(model_name=vm,eqtags=['aux','unused'],targets=['unused']);\n";
    let source = format!("{VAR}{extra}");
    let result = fragment(&source, "wanted");
    let parsed = parse(result.fragment.as_deref().unwrap());
    assert!(parsed.semi_structural_commands.iter().any(
        |command| command.kind == crate::engine::language::model::SemiStructuralKind::VarModel
    ));
    assert!(
        !parsed
            .semi_structural_commands
            .iter()
            .any(|command| command.kind
                == crate::engine::language::model::SemiStructuralKind::TrendComponentModel)
    );
    let source = format!("{PAC}{extra}");
    let result = fragment(&source, "wanted");
    let parsed = parse(result.fragment.as_deref().unwrap());
    assert!(
        parsed
            .semi_structural_commands
            .iter()
            .any(|command| command.kind
                == crate::engine::language::model::SemiStructuralKind::TrendComponentModel)
    );
    assert!(!parsed.semi_structural_commands.iter().any(
        |command| command.kind == crate::engine::language::model::SemiStructuralKind::VarModel
    ));
}

#[test]
fn named_commands_stay_between_source_order_type_changes() {
    // Guards moving a named declaration across the type change that determines its option's role.
    let source = "var x y;\nvarexo e;\nvar betta;\nchange_type(parameters) betta;\npac_model(model_name=pm,discount=betta);\nchange_type(var) betta;\nmodel;\n[name='wanted'] y=betta+pac_expectation(pm);\n[name='x'] x=x(-1)+e;\n[name='b'] betta=0;\nend;\n";
    accepted_at_check(source);
    let result = fragment(source, "wanted");
    let text = result.fragment.as_deref().unwrap();
    accepted_at_check(text);
    let parsed = parse(text);
    let command = &parsed.semi_structural_commands[0];
    let betta = parsed
        .parameters
        .iter()
        .chain(&parsed.endogenous)
        .find(|declaration| parsed.name(declaration.name) == "betta")
        .unwrap()
        .name;
    assert_eq!(
        parsed.symbol_kind_in_context(betta, command.symbol_type_context),
        Some("parameters")
    );
    assert!(
        parsed
            .final_endogenous()
            .iter()
            .any(|declaration| parsed.name(declaration.name) == "betta")
    );
}

#[test]
fn omitted_native_initialization_and_unrelated_models_are_explained() {
    // Guards silently dropping the original runtime setup or unrelated named definitions.
    let source = format!(
        "{VAR}var_model(model_name=unrelated,eqtags=['unused']);\nvar_expectation.initialize('ve');\n"
    );
    let result = fragment(&source, "wanted");
    assert!(
        result
            .omitted_context
            .iter()
            .any(|note| note.kind == "native" && note.detail.contains("initialization"))
    );
    assert!(
        result
            .omitted_context
            .iter()
            .any(|note| note.kind == "named_models" && note.detail.contains("unrelated"))
    );
    let parsed = parse(result.fragment.as_deref().unwrap());
    assert!(!parsed.execution_steps.iter().any(|step| matches!(
        step,
        crate::engine::language::model::ExecutionStep::Opaque(_)
    )));
}
