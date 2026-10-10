//! Named retained-family facts and conservative occurrence acceptance.

use dygnosis::model_diff::{compare_models, ModelDiff};
use dygnosis::semantic_diff::{ChangeKind, CountUnit, FieldValue, SemanticFamily};
use dygnosis::{check_parse, parse};

const BASE: &str = "var y c x v; varexo e u; parameters a b; a=.5; b=.9; model; y=a*y(-1)+e; c=y; x=c+u; v=x; end;\n";

fn comparison(before: &str, after: &str) -> ModelDiff {
    let old = parse(before);
    let new = parse(after);
    assert!(
        check_parse(&old).is_empty(),
        "Before: {:?}",
        check_parse(&old)
    );
    assert!(
        check_parse(&new).is_empty(),
        "After: {:?}",
        check_parse(&new)
    );
    compare_models(&old, &new)
}

fn assert_case(family: SemanticFamily, field: &str, before: &str, after: &str) -> ModelDiff {
    let control = comparison(before, before);
    assert!(
        control.semantic.rows.iter().all(|row| row.family != family),
        "unchanged {family:?}: {}",
        control.to_json()
    );
    let diff = comparison(before, after);
    assert!(
        diff.semantic
            .rows
            .iter()
            .filter(|row| row.family == family)
            .any(|row| row.fields.iter().any(|value| value.name == field
                && value.changed
                && (value.before.value.is_some() || value.after.value.is_some()))),
        "missing changed {family:?}.{field}: {}",
        diff.to_json()
    );
    diff
}

fn appended(family: SemanticFamily, field: &str, before: &str, after: &str) -> ModelDiff {
    assert_case(
        family,
        field,
        &format!("{BASE}{before}"),
        &format!("{BASE}{after}"),
    )
}

#[test]
fn observables_roles_and_duplicate_history_stay_separate() {
    let diff = appended(
        SemanticFamily::Observables,
        "target",
        "varobs y c; varobs y;",
        "varobs y x; varobs y;",
    );
    assert!(
        diff.semantic
            .rows
            .iter()
            .filter(|row| row.family == SemanticFamily::Observables)
            .all(|row| row.change != ChangeKind::Changed),
        "span-only records cannot invent occurrence pairing"
    );
    appended(
        SemanticFamily::Observables,
        "target",
        "varexobs e;",
        "varexobs u;",
    );
}

#[test]
fn observation_trends_keep_target_and_expression_gap() {
    appended(
        SemanticFamily::Observables,
        "target",
        "observation_trends; y(a); end;",
        "observation_trends; c(a); end;",
    );
    let diff = comparison(
        &format!("{BASE}observation_trends; y(a); end;"),
        &format!("{BASE}observation_trends; y(b); end;"),
    );
    assert!(diff
        .coverage
        .families
        .iter()
        .filter(|family| family.family == SemanticFamily::Observables)
        .flat_map(|family| &family.limits)
        .any(|limit| limit.code == "observation_trend_expression_not_retained"));
    assert!(diff
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Commands));
}

#[test]
fn database_and_dates_compare_retained_names_and_text() {
    appended(
        SemanticFamily::Data,
        "variables",
        "database D1 D2;",
        "database D1 D3;",
    );
    appended(
        SemanticFamily::Data,
        "date",
        "set_time(2000Q1);",
        "set_time(2000Q2);",
    );
    appended(
        SemanticFamily::Data,
        "data_options",
        "estimation(first_obs=2000Q1);",
        "estimation(first_obs=2000Q1+1);",
    );
}

#[test]
fn subsample_labels_endpoints_and_copy_heads_remain_associated() {
    let diff = appended(
        SemanticFamily::Data,
        "ranges",
        "a.subsamples(s=2000Q1:2001Q1,t=2002Q1:2003Q1);",
        "a.subsamples(s=2000Q1:2001Q2,t=2002Q1:2003Q1);",
    );
    let json = diff.to_json().to_string();
    assert!(json.contains("2001Q2") && json.contains("2002Q1") && json.contains("label"));
    appended(
        SemanticFamily::Data,
        "source",
        "a.subsamples(s=2000Q1:2001Q1); b.subsamples=a.subsamples;",
        "a.subsamples(s=2000Q1:2001Q1); b.subsamples=b.subsamples;",
    );
}

#[test]
fn data_options_and_estimation_forms_compare_named_fields() {
    let diff = appended(
        SemanticFamily::Data,
        "options",
        "data(file='a.csv', nobs=10);",
        "data(file='b.csv', nobs=10);",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Data && row.name == "data")
        .any(|row| row.change == ChangeKind::Changed));
    appended(
        SemanticFamily::Data,
        "has_datafile",
        "estimation(datafile=a);",
        "estimation;",
    );
    appended(
        SemanticFamily::Data,
        "estimated_form",
        "estimation(dsge_var);",
        "estimation(dsge_var=.4);",
    );
    appended(
        SemanticFamily::Data,
        "filename",
        "load_params_and_steady_state('a.mat');",
        "load_params_and_steady_state('b.mat');",
    );
}

#[test]
fn occbin_conditions_compare_exact_retained_text() {
    let diff = appended(
        SemanticFamily::Occbin,
        "bind",
        "occbin_constraints; name 'ELB'; bind y<0; relax y>1; end;",
        "occbin_constraints; name 'ELB'; bind y<.1; relax y>1; end;",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Occbin)
        .flat_map(|row| &row.expressions)
        .any(|detail| detail.field == "bind"));
}

#[test]
fn policy_command_instruments_and_first_planner_values_are_distinct() {
    appended(
        SemanticFamily::Policy,
        "instruments",
        "ramsey_model(instruments=(y), planner_discount=.9);",
        "ramsey_model(instruments=(c), planner_discount=.9);",
    );
    appended(
        SemanticFamily::Policy,
        "planner_discount_value",
        "ramsey_model(planner_discount=.9);",
        "ramsey_model(planner_discount=.8);",
    );
    appended(
        SemanticFamily::Policy,
        "planner_objective",
        "planner_objective y^2;",
        "planner_objective y^3;",
    );
}

#[test]
fn policy_bounds_weights_and_constraints_compare_owned_trees() {
    appended(
        SemanticFamily::Policy,
        "upper",
        "osr_params_bounds; a,0,1; end;",
        "osr_params_bounds; a,0,2; end;",
    );
    appended(
        SemanticFamily::Policy,
        "weight",
        "optim_weights; y, c 1; end;",
        "optim_weights; y, c 2; end;",
    );
    let diff = appended(
        SemanticFamily::Policy,
        "constraint",
        "ramsey_constraints; y>0; end;",
        "ramsey_constraints; y>1; end;",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Policy)
        .flat_map(|row| &row.fields)
        .any(|field| matches!(
            field.before.value.as_ref().or(field.after.value.as_ref()),
            Some(FieldValue::List(_))
        )));
}

#[test]
fn semi_structural_options_preserve_shapes_and_pac_components() {
    appended(
        SemanticFamily::SemiStructural,
        "options",
        "var_model(model_name=aux,eqtags=['y','c']);",
        "var_model(model_name=aux,eqtags=['c','y']);",
    );
    appended(SemanticFamily::SemiStructural, "options", "var_expectation_model(model_name=forecast,expression=y+c(-1),auxiliary_model_name=aux,horizon=1:Inf);", "var_expectation_model(model_name=forecast,expression=y+c(-2),auxiliary_model_name=aux,horizon=1:Inf);");
    let diff = appended(SemanticFamily::SemiStructural, "rows", "pac_target_info(pac); target v; component y; kind ll; auxname ya; component x; kind dd; auxname xa; end;", "pac_target_info(pac); target v; component y; kind ll; auxname ya; component x; kind dl; auxname xa; end;");
    assert!(diff.to_json().to_string().contains("xa"));
}

#[test]
fn method_and_matched_moments_compare_named_expression_fields() {
    appended(
        SemanticFamily::Moments,
        "options",
        "method_of_moments(mom_method=GMM);",
        "method_of_moments(mom_method=SMM);",
    );
    appended(
        SemanticFamily::Moments,
        "expression",
        "matched_moments; y*y(-1); end;",
        "matched_moments; y*y(-2); end;",
    );
}

#[test]
fn irf_settings_weights_and_calibration_associations_compare() {
    appended(
        SemanticFamily::Moments,
        "overwrite",
        "matched_irfs; var y; varexo e; periods 1; values 1; end;",
        "matched_irfs(overwrite); var y; varexo e; periods 1; values 1; end;",
    );
    appended(
        SemanticFamily::Moments,
        "rows",
        "matched_irfs_weights; y(1), e, c(2), u, .5; end;",
        "matched_irfs_weights; y(1), e, c(2), u, .6; end;",
    );
    appended(
        SemanticFamily::Moments,
        "rows",
        "moment_calibration; y,c(-2:2),[0,1]; end;",
        "moment_calibration; y,c(-2:2),[0,2]; end;",
    );
    appended(
        SemanticFamily::Moments,
        "relative_irf",
        "irf_calibration; y(1:2),e,+; end;",
        "irf_calibration(relative_irf); y(1:2),e,+; end;",
    );
    appended(
        SemanticFamily::Moments,
        "exogenous",
        "generate_irfs; scenario,e=1; end;",
        "generate_irfs; scenario,u=1; end;",
    );
}

#[test]
fn ms_options_keep_matrix_shape_text_and_order() {
    let diff = appended(
        SemanticFamily::MsSbvar,
        "options",
        "markov_switching(chain=1,number_of_regimes=2,duration=2.5,parameters=[a,b]);",
        "markov_switching(chain=1,number_of_regimes=2,duration=2.5,parameters=[b,a]);",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::MsSbvar)
        .any(|row| row.change == ChangeKind::Changed));
    appended(
        SemanticFamily::MsSbvar,
        "options",
        "sbvar(coefficient_prior_hyperparameters=[1,2,3]);",
        "sbvar(coefficient_prior_hyperparameters=[1,3,2]);",
    );
}

#[test]
fn svar_and_conditional_paths_use_proven_element_order() {
    appended(
        SemanticFamily::MsSbvar,
        "lag",
        "svar_identification; exclusion lag 0; equation 1,y,c; end;",
        "svar_identification; exclusion lag 1; equation 1,y,c; end;",
    );
    let diff = appended(
        SemanticFamily::MsSbvar,
        "values",
        "conditional_forecast_paths; var y; periods 1 2; values 3 4; end;",
        "conditional_forecast_paths; var y; periods 1 2; values 4 3; end;",
    );
    assert!(diff.to_json().to_string().contains("values"));
}

#[test]
fn heterogeneity_dimensions_and_command_options_compare() {
    assert_case(
        SemanticFamily::Heterogeneity,
        "dimension",
        "heterogeneity_dimension d;",
        "heterogeneity_dimension h;",
    );
    appended(
        SemanticFamily::Heterogeneity,
        "options",
        "heterogeneity_compute_steady_state(filename='a.mat');",
        "heterogeneity_compute_steady_state(filename='b.mat');",
    );
    appended(
        SemanticFamily::Heterogeneity,
        "simulate_names",
        "heterogeneity_simulate y c;",
        "heterogeneity_simulate y x;",
    );
}

#[test]
fn external_interface_keeps_omitted_and_explicit_default_distinct() {
    appended(
        SemanticFamily::ExternalFunctions,
        "nargs",
        "external_function(name=f);",
        "external_function(name=f,nargs=1);",
    );
    let diff = appended(
        SemanticFamily::ExternalFunctions,
        "first_derivative",
        "external_function(name=f,first_deriv_provided);",
        "external_function(name=f,first_deriv_provided=df);",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::ExternalFunctions)
        .all(|row| row.count_unit == CountUnit::AcceptedOccurrence));
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family != SemanticFamily::Commands),
        "fully retained external interface must claim supporting context: {}",
        diff.to_json()
    );
}

#[test]
fn trends_compare_growth_deflator_and_ordered_deterministic_rows() {
    appended(
        SemanticFamily::Trends,
        "growth",
        "trend_var(growth_factor=1.02) A;",
        "trend_var(growth_factor=1.03) A;",
    );
    appended(
        SemanticFamily::Trends,
        "deflator",
        "trend_var(growth_factor=1.02) A; var(deflator=A) z;",
        "trend_var(growth_factor=1.02) A; var(deflator=A^2) z;",
    );
    appended(
        SemanticFamily::Trends,
        "rows",
        "deterministic_trends; y(a); c(b); end;",
        "deterministic_trends; y(b); c(a); end;",
    );
}

#[test]
fn epilogue_filter_and_homotopy_keep_written_operation_roles() {
    appended(
        SemanticFamily::Operations,
        "expression",
        "epilogue; output=1; end;",
        "epilogue; output=2; end;",
    );
    appended(
        SemanticFamily::Operations,
        "lag",
        "filter_initial_state; y(-1)=1; end;",
        "filter_initial_state; y(-2)=1; end;",
    );
    appended(
        SemanticFamily::Operations,
        "expression",
        "filter_initial_state; y(-1)=1; end;",
        "filter_initial_state; y(-1)=2; end;",
    );
    appended(
        SemanticFamily::Operations,
        "target",
        "homotopy_setup; a,0,1; end;",
        "homotopy_setup; b,0,1; end;",
    );
}

#[test]
fn type_surgery_and_pruning_count_operations_separately() {
    let diff = assert_case(
        SemanticFamily::Operations,
        "new_type",
        "parameters a; change_type(var) a;",
        "parameters a; change_type(varexo) a;",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Operations)
        .all(|row| row.count_unit == CountUnit::Operation));
    appended(
        SemanticFamily::Operations,
        "target",
        "var_remove y;",
        "var_remove c;",
    );
    let old = "var y c; initval; y=1; c=2; end; model; [name='y',endogenous='y'] y=0; [name='c',endogenous='c'] c=0; end; model_remove('y');";
    let new = old.replace("model_remove('y')", "model_remove('c')");
    let diff = assert_case(SemanticFamily::Operations, "tag_sets", old, &new);
    assert!(diff
        .semantic
        .rows
        .iter()
        .flat_map(|row| &row.limits)
        .any(|limit| limit.code == "pruned_initialization_rhs_not_retained"));
}

#[test]
fn equation_surgery_written_source_has_no_extra_terminator_command() {
    let old = "var c; model; [name='Consumption'] c=.8; end;";
    let new = "var c; model; [name='Consumption'] c=1; end; model_replace('Consumption'); [name='Consumption'] c=.8; end;";
    let diff = comparison(old, new);
    assert!(diff.changed_equations.is_empty());
    assert!(diff
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Operations && row.name == "model_replace"));
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family != SemanticFamily::Commands),
        "owned replacement equations and block delimiters must not create a Commands row"
    );
}

#[test]
fn written_macro_context_does_not_claim_expanded_occurrence_pairs() {
    assert_case(
        SemanticFamily::MacroContext,
        "directives",
        "@#define n=1\nparameters p; p=@{n};",
        "@#define n=2\nparameters p; p=@{n};",
    );
    let diff = appended(
        SemanticFamily::Occbin,
        "bind",
        "@#for i in 1:2\noccbin_constraints; name 'ELB'; bind y<0; end;\n@#endfor",
        "@#for i in 1:2\noccbin_constraints; name 'ELB'; bind y<1; end;\n@#endfor",
    );
    let rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Occbin)
        .collect();
    assert_eq!(rows.len(), 4);
    assert!(rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::Commands));
}

#[test]
fn selected_presence_and_numeric_settings_keep_history_limits() {
    appended(
        SemanticFamily::Data,
        "bayesian_irf",
        "estimation;",
        "estimation(bayesian_irf);",
    );
    let diff = appended(
        SemanticFamily::MsSbvar,
        "identification_order",
        "identification(order=1);",
        "identification(order=2);",
    );
    assert!(diff
        .semantic
        .rows
        .iter()
        .flat_map(|row| &row.limits)
        .any(|limit| limit.code == "identification_order_retained_history"));
}

#[test]
fn one_changed_span_only_family_does_not_emit_unrelated_controls() {
    let old =
        format!("{BASE}varobs y; set_time(2000Q1); occbin_constraints; name 'ELB'; bind y<0; end;");
    let new = old.replace("y<0", "y<1");
    let diff = comparison(&old, &new);
    assert!(diff.semantic.rows.iter().all(|row| !matches!(
        row.family,
        SemanticFamily::Observables | SemanticFamily::Data
    )));
}

#[test]
fn retained_include_fact_uses_expanded_order_and_macro_copies_stay_separate() {
    fn included(body: &str) -> dygnosis::Model {
        let root = "C:/dygnosis-semantic-families/root.mod";
        let child = "C:/dygnosis-semantic-families/interface.inc";
        let mut workspace = dygnosis::Workspace::new();
        workspace.update_document(child, body);
        workspace.update_document(root, "@#include \"interface.inc\"\n");
        assert!(workspace.includes_complete(root));
        workspace.get_effective_model(root).unwrap().clone()
    }
    let old = included("external_function(name=f,nargs=1);");
    let new = included("external_function(name=f,nargs=2);");
    let control = compare_models(&old, &old);
    assert!(control
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::ExternalFunctions));
    let diff = compare_models(&old, &new);
    let row = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::ExternalFunctions)
        .unwrap();
    assert_eq!(row.change, ChangeKind::Changed);
    assert!(row.before.as_ref().unwrap().context.is_some());
    assert!(row.after.as_ref().unwrap().context.is_some());
    assert!(row
        .fields
        .iter()
        .any(|field| field.name == "nargs" && field.changed));
}

#[test]
fn missing_homotopy_endpoints_and_irf_coefficients_keep_accepted_body_context() {
    for (old, new, code) in [
        (
            "homotopy_setup; a,0,1; end;",
            "homotopy_setup; a,0,2; end;",
            "homotopy_endpoints_not_retained",
        ),
        (
            "generate_irfs; scenario,e=1; end;",
            "generate_irfs; scenario,e=2; end;",
            "generate_irf_coefficients_not_retained",
        ),
    ] {
        let diff = comparison(&format!("{BASE}{old}"), &format!("{BASE}{new}"));
        assert!(diff
            .coverage
            .families
            .iter()
            .flat_map(|family| &family.limits)
            .any(|limit| limit.code == code));
        assert!(
            diff.semantic
                .rows
                .iter()
                .any(|row| row.family == SemanticFamily::Commands),
            "{old}: {}",
            diff.to_json()
        );
    }
}

#[test]
fn command_lists_and_irf_requests_keep_named_ordered_fields() {
    appended(
        SemanticFamily::Commands,
        "symbols",
        "stoch_simul y c;",
        "stoch_simul y x;",
    );
    appended(
        SemanticFamily::Policy,
        "symbols",
        "osr_params a b;",
        "osr_params b a;",
    );
    appended(
        SemanticFamily::Shocks,
        "irf",
        "stoch_simul(irf=0);",
        "stoch_simul(irf=1);",
    );
    appended(
        SemanticFamily::Shocks,
        "irf_shocks",
        "stoch_simul(irf_shocks=(e));",
        "stoch_simul(irf_shocks=(u));",
    );
    appended(
        SemanticFamily::Shocks,
        "irf_shocks",
        "estimation(irf_shocks=(e));",
        "estimation(irf_shocks=(u));",
    );
}

#[test]
fn command_list_proof_keeps_options_diagnostics_and_macro_ambiguity() {
    let list_rows = |diff: &ModelDiff| {
        diff.semantic
            .rows
            .iter()
            .filter(|row| {
                row.fields.iter().any(|field| {
                    field.name == "role"
                        && field.before.value.as_ref().or(field.after.value.as_ref())
                            == Some(&FieldValue::Text("command_symbol_list".into()))
                })
            })
            .count()
    };
    let diff = comparison(
        &format!("{BASE}stoch_simul(order=1) y;"),
        &format!("{BASE}stoch_simul(order=1) c;"),
    );
    assert_eq!(list_rows(&diff), 1);
    assert_eq!(
        diff.semantic
            .rows
            .iter()
            .filter(|row| row.family == SemanticFamily::Commands)
            .count(),
        1
    );
    let row = diff
        .semantic
        .rows
        .iter()
        .find(|row| row.family == SemanticFamily::Commands)
        .unwrap();
    assert_eq!(
        row.before.as_ref().unwrap().context.as_ref().unwrap().name,
        "stoch_simul"
    );
    let options = comparison(
        &format!("{BASE}stoch_simul(order=1) y;"),
        &format!("{BASE}stoch_simul(order=2) y;"),
    );
    assert_eq!(list_rows(&options), 0);
    assert!(options
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Commands));

    let old = format!("{BASE}@#for i in 1:2\nstoch_simul y;\n@#endfor\n");
    let new = old.replace("stoch_simul y", "stoch_simul c");
    assert_eq!(list_rows(&comparison(&old, &old)), 0);
    let repeated = comparison(&old, &new);
    assert_eq!(list_rows(&repeated), 4);
    assert!(repeated.semantic.rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));

    let old = parse(&format!("{BASE}stoch_simul ghost;"));
    let new = parse(&format!("{BASE}stoch_simul(order=1) ghost;"));
    let codes = |model: &dygnosis::Model| {
        dygnosis::analyze(model)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>()
    };
    assert!(!codes(&old).is_empty());
    assert_eq!(codes(&old), codes(&new));
}

#[test]
fn unmatched_surgery_targets_are_retained_without_guessing_effects() {
    let diff = appended(
        SemanticFamily::Operations,
        "unmatched_tag_sets",
        "model_remove('missing');",
        "model_remove('other');",
    );
    assert!(diff.changed_equations.is_empty());
    assert!(diff.added_equations.is_empty());
    assert!(diff.removed_equations.is_empty());
}

#[test]
fn refused_filter_rows_stay_unavailable_while_accepted_children_compare() {
    let old = parse(&format!(
        "{BASE}filter_initial_state; y(-1) 2; c(-1)=3; end;"
    ));
    let new = parse(&format!(
        "{BASE}filter_initial_state; y(-1) 2; c(-1)=4; end;"
    ));
    let codes = |model: &dygnosis::Model| {
        dygnosis::analyze(model)
            .into_iter()
            .map(|diagnostic| diagnostic.code)
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(&old), codes(&new));
    let diff = compare_models(&old, &new);
    let rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| {
            row.family == SemanticFamily::Operations
                && row.fields.iter().any(|field| {
                    field.name == "role"
                        && field.before.value.as_ref().or(field.after.value.as_ref())
                            == Some(&FieldValue::Text("filter_initial_state".into()))
                })
        })
        .collect();
    assert!(!rows.is_empty());
    assert!(rows.iter().all(|row| row.name == "c"));
    assert!(!diff.semantic.rows.iter().any(|row| {
        row.family == SemanticFamily::Commands && row.name == "filter_initial_state"
    }));
    assert!(rows
        .iter()
        .flat_map(|row| &row.fields)
        .any(|field| field.name == "expression" && field.changed));
}

fn assert_one_owner(diff: &ModelDiff, family: SemanticFamily, field: &str) {
    assert_eq!(
        diff.semantic
            .rows
            .iter()
            .filter(|row| row.family == family
                && row
                    .fields
                    .iter()
                    .any(|value| value.name == field && value.changed))
            .count(),
        1,
        "one {family:?}.{field} owner: {}",
        diff.to_json()
    );
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family != SemanticFamily::Commands),
        "duplicate command context: {}",
        diff.to_json()
    );
}

#[test]
fn written_declarations_survive_the_same_final_retype() {
    let diff = assert_case(
        SemanticFamily::Symbols,
        "written_kind",
        "var x; change_type(parameters) x;",
        "varexo x; change_type(parameters) x;",
    );
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family == SemanticFamily::Symbols),
        "{}",
        diff.to_json()
    );
    assert_eq!(diff.semantic.rows.len(), 2);
    assert!(diff.semantic.rows.iter().all(|row| row.change
        == if row.before.is_some() {
            ChangeKind::Removed
        } else {
            ChangeKind::Added
        }));
    assert!(diff.symbols_changed.is_empty());
    let old = "heterogeneity_dimension d e; var(heterogeneity=d) x; change_type(parameters) x;";
    let new = old.replace("heterogeneity=d", "heterogeneity=e");
    let diff = assert_case(SemanticFamily::Symbols, "written_dimension", old, &new);
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family == SemanticFamily::Symbols),
        "{}",
        diff.to_json()
    );
    assert_one_owner(&diff, SemanticFamily::Symbols, "written_dimension");
    assert!(diff.symbols_changed.is_empty());
}

#[test]
fn removed_written_declarations_keep_metadata_and_log_flags() {
    for (field, old, new) in [
        (
            "long_name",
            "var x(long_name='a'); var_remove x;",
            "var x(long_name='b'); var_remove x;",
        ),
        (
            "tex_name",
            "var x $a$; var_remove x;",
            "var x $b$; var_remove x;",
        ),
        (
            "log_transform",
            "var x; var_remove x;",
            "var(log) x; var_remove x;",
        ),
    ] {
        let diff = assert_case(SemanticFamily::Symbols, field, old, new);
        assert!(
            diff.semantic
                .rows
                .iter()
                .all(|row| row.family != SemanticFamily::Commands),
            "{}",
            diff.to_json()
        );
    }
}

#[test]
fn mixed_matched_irfs_keep_each_ordered_item_and_range() {
    let old = "matched_irfs; var y; varexo e; periods 1,2:4; values 1,(a); weights (.5),2; end;";
    for new in [
        old.replace("values 1,(a)", "values 2,(a)"),
        old.replace("values 1,(a)", "values 1,(b)"),
        old.replace("weights (.5),2", "weights (.5),3"),
        old.replace("weights (.5),2", "weights (.6),2"),
        old.replace("periods 1,2:4", "periods 1,2:5"),
        old.replace("periods 1,2:4", "periods 1,2,4"),
    ] {
        let diff = appended(SemanticFamily::Moments, "rows", old, &new);
        assert_one_owner(&diff, SemanticFamily::Moments, "rows");
        assert!(diff
            .semantic
            .rows
            .iter()
            .flat_map(|row| &row.limits)
            .any(|limit| limit.code == "matched_irf_expression_positions"));
    }
}

#[test]
fn direct_condition_request_and_selected_setting_receipts_have_one_owner() {
    for (family, field, old, new) in [
        (
            SemanticFamily::Occbin,
            "bind",
            "occbin_constraints; name 'ELB'; bind y<0; relax y>1; end;",
            "occbin_constraints; name 'ELB'; bind y<.1; relax y>1; end;",
        ),
        (
            SemanticFamily::Shocks,
            "irf",
            "stoch_simul(irf=0);",
            "stoch_simul(irf=1);",
        ),
        (
            SemanticFamily::Shocks,
            "irf",
            "stoch_simul;",
            "stoch_simul(irf=1);",
        ),
        (
            SemanticFamily::Operations,
            "is_linear",
            "model; y=0; end;",
            "model(linear); y=0; end;",
        ),
        (
            SemanticFamily::MsSbvar,
            "identification_order",
            "identification(order=1);",
            "identification(order=2);",
        ),
        (
            SemanticFamily::Policy,
            "discretionary_order",
            "discretionary_policy(order=1);",
            "discretionary_policy(order=2);",
        ),
        (
            SemanticFamily::Policy,
            "planner_discount_value",
            "ramsey_model(planner_discount=.9);",
            "ramsey_model(planner_discount=.8);",
        ),
        (
            SemanticFamily::Policy,
            "planner_objective",
            "planner_objective var_expectation(foo);",
            "planner_objective var_expectation(bar);",
        ),
        (
            SemanticFamily::Policy,
            "upper",
            "osr_params_bounds; a,0,1; end;",
            "osr_params_bounds; a,0,2; end;",
        ),
        (
            SemanticFamily::Policy,
            "weight",
            "optim_weights; y,c 1; end;",
            "optim_weights; y,c 2; end;",
        ),
        (
            SemanticFamily::Policy,
            "constraint",
            "ramsey_constraints; y>0; end;",
            "ramsey_constraints; y>1; end;",
        ),
    ] {
        let diff = appended(family, field, old, new);
        assert_one_owner(&diff, family, field);
    }
    let old = format!("{BASE}stoch_simul(irf=0,order=1);");
    let new = old.replace("irf=0,order=1", "irf=1,order=2");
    let diff = comparison(&old, &new);
    assert!(diff
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Shocks));
    assert!(diff
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Commands));
    let old = "var y; model(linear,block); y=0; end;";
    let new = "var y; model; y=0; end;";
    let diff = comparison(old, new);
    assert!(
        diff.semantic
            .rows
            .iter()
            .all(|row| row.family != SemanticFamily::Commands),
        "{}",
        diff.to_json()
    );
}

#[test]
fn presence_receipts_preserve_unretained_values_and_arguments() {
    let compact = |text: &str| {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>()
    };
    for (old, new) in [
        ("estimation(bayesian_irf=0);", "estimation(bayesian_irf=1);"),
        (
            "estimation(mh_tune_jscale=0);",
            "estimation(mh_tune_jscale=1);",
        ),
        (
            "estimation(mh_tune_jscale(0));",
            "estimation(mh_tune_jscale(1));",
        ),
        ("stoch_simul(hp_filter=1);", "stoch_simul(hp_filter=2);"),
        (
            "model(linear,cutoff=.1); y=0; end;",
            "model(linear,cutoff=.2); y=0; end;",
        ),
        (
            "ramsey_model(planner_discount=.9); ramsey_model(planner_discount=.8);",
            "ramsey_model(planner_discount=.9); ramsey_model(planner_discount=.7);",
        ),
    ] {
        let diff = comparison(&format!("{BASE}{old}"), &format!("{BASE}{new}"));
        assert!(
            diff.semantic
                .rows
                .iter()
                .any(|row| row.family == SemanticFamily::Commands
                    || old.starts_with("estimation(")
                        && row.family == SemanticFamily::Data
                        && row.expressions.iter().any(|expression| {
                            expression.field == "statement_tokens"
                                && expression
                                    .before
                                    .as_ref()
                                    .zip(expression.after.as_ref())
                                    .is_some_and(|(before, after)| {
                                        before.text != after.text
                                            && compact(old).contains(&compact(&before.text))
                                            && compact(new).contains(&compact(&after.text))
                                    })
                        })),
            "lost unretained option {old}: {}",
            diff.to_json()
        );
    }
}

#[test]
fn removed_equation_metadata_has_one_surgery_owner() {
    let old = "var y; model; [name='eq',custom='a'] y=0; end; model_remove('eq');";
    let new = old.replace("custom='a'", "custom='b'");
    let diff = assert_case(SemanticFamily::Operations, "removed_equations", old, &new);
    assert_one_owner(&diff, SemanticFamily::Operations, "removed_equations");
    assert!(diff.changed_equations.is_empty());
    assert!(diff.to_json().to_string().contains("tag_map"));
}

#[test]
fn complete_text_and_typed_row_owners_claim_their_direct_receipts() {
    for (family, field, old, new) in [
        (
            SemanticFamily::Moments,
            "expression",
            "matched_moments; y*y(-1); end;",
            "matched_moments; y*y(-2); end;",
        ),
        (
            SemanticFamily::Moments,
            "rows",
            "matched_irfs_weights; y(1),e,c(2),u,.5; end;",
            "matched_irfs_weights; y(1),e,c(2),u,.6; end;",
        ),
        (
            SemanticFamily::Moments,
            "rows",
            "moment_calibration; y,c(-2:2),[0,1]; end;",
            "moment_calibration; y,c(-2:2),[0,2]; end;",
        ),
        (
            SemanticFamily::Moments,
            "relative_irf",
            "irf_calibration; y(1:2),e,+; end;",
            "irf_calibration(relative_irf); y(1:2),e,+; end;",
        ),
        (
            SemanticFamily::SemiStructural,
            "options",
            "var_model(model_name=aux,eqtags=['y','c']);",
            "var_model(model_name=aux,eqtags=['c','y']);",
        ),
        (
            SemanticFamily::SemiStructural,
            "rows",
            "pac_target_info(pac); target v; component y; kind ll; auxname ya; end;",
            "pac_target_info(pac); target v; component y; kind dl; auxname ya; end;",
        ),
        (
            SemanticFamily::Trends,
            "rows",
            "deterministic_trends; y(1.01); end;",
            "deterministic_trends; y(1.02); end;",
        ),
        (
            SemanticFamily::Policy,
            "instruments",
            "ramsey_model(instruments=(y),planner_discount=.9);",
            "ramsey_model(instruments=(c),planner_discount=.9);",
        ),
        (
            SemanticFamily::Operations,
            "initval_all_values_required",
            "initval; y=1; end;",
            "initval(all_values_required); y=1; end;",
        ),
        (
            SemanticFamily::Operations,
            "endval_all_values_required",
            "endval; y=1; end;",
            "endval(all_values_required); y=1; end;",
        ),
    ] {
        let diff = appended(family, field, old, new);
        assert!(
            diff.semantic
                .rows
                .iter()
                .all(|row| row.family != SemanticFamily::Commands),
            "{old}: {}",
            diff.to_json()
        );
    }
}

#[test]
fn target_date_and_subsample_receipts_do_not_count_command_context_twice() {
    for (family, field, old, new) in [
        (
            SemanticFamily::Observables,
            "target",
            "varobs y c;",
            "varobs y x;",
        ),
        (
            SemanticFamily::Observables,
            "target",
            "varexobs e;",
            "varexobs u;",
        ),
        (
            SemanticFamily::Observables,
            "target",
            "observation_trends; y(a); end;",
            "observation_trends; c(a); end;",
        ),
        (
            SemanticFamily::Data,
            "variables",
            "database D1 D2;",
            "database D1 D3;",
        ),
        (
            SemanticFamily::Data,
            "date",
            "set_time(2000Q1);",
            "set_time(2000Q2);",
        ),
        (
            SemanticFamily::Data,
            "data_options",
            "estimation(first_obs=2000Q1);",
            "estimation(first_obs=2000Q2);",
        ),
        (
            SemanticFamily::Data,
            "ranges",
            "a.subsamples(s=2000Q1:2001Q1,t=2002Q1:2003Q1);",
            "a.subsamples(s=2000Q1:2001Q2,t=2002Q1:2003Q1);",
        ),
        (
            SemanticFamily::Data,
            "source",
            "a.subsamples(s=2000Q1:2001Q1); b.subsamples=a.subsamples;",
            "a.subsamples(s=2000Q1:2001Q1); b.subsamples=b.subsamples;",
        ),
        (
            SemanticFamily::Heterogeneity,
            "options",
            "heterogeneity_compute_steady_state(filename='a.mat');",
            "heterogeneity_compute_steady_state(filename='b.mat');",
        ),
        (
            SemanticFamily::Heterogeneity,
            "simulate_names",
            "heterogeneity_simulate y c;",
            "heterogeneity_simulate y x;",
        ),
        (
            SemanticFamily::Data,
            "filename",
            "load_params_and_steady_state('a.mat');",
            "load_params_and_steady_state('b.mat');",
        ),
    ] {
        let diff = appended(family, field, old, new);
        assert!(
            diff.semantic
                .rows
                .iter()
                .all(|row| row.family != SemanticFamily::Commands),
            "{old}: {}",
            diff.to_json()
        );
    }
    let old = format!("{BASE}estimation(first_obs=2000Q1,order=1);");
    let new = old.replace("order=1", "order=2");
    let diff = comparison(&old, &new);
    let rows: Vec<_> = diff
        .semantic
        .rows
        .iter()
        .filter(|row| row.family == SemanticFamily::Data)
        .collect();
    assert_eq!(rows.len(), 1);
    let text = rows[0]
        .expressions
        .iter()
        .find(|expression| expression.field == "statement_tokens")
        .unwrap();
    assert!(text.before.as_ref().unwrap().text.contains("order = 1"));
    assert!(text.after.as_ref().unwrap().text.contains("order = 2"));
    assert!(rows[0]
        .fields
        .iter()
        .any(|field| field.name == "data_options" && !field.changed));
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::Commands));
    let old = format!(
        "{BASE}load_params_and_steady_state('a.mat'); load_params_and_steady_state('b.mat');"
    );
    let new = old.replace("'b.mat'", "'c.mat'");
    let diff = comparison(&old, &new);
    assert!(diff
        .semantic
        .rows
        .iter()
        .all(|row| row.family != SemanticFamily::Data));
    assert!(diff
        .semantic
        .rows
        .iter()
        .any(|row| row.family == SemanticFamily::Commands));
}
