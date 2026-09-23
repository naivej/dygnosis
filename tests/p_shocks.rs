use std::time::Duration;

use dygnosis::model::{
    DottedHead, DottedKind, PathTarget, PeriodPoint, ShockBlockKind, ShockOperation,
    SubsampleInstruction,
};
use dygnosis::{analyze, find_preprocessor, parse, run_preprocessor, JsonStage};

const BASE: &str = "var y; varexo e; varexo_det d; parameters p; p=.5; model; y=p*y(-1)+e+d; end;";

fn file(body: &str) -> String {
    format!("{BASE} {body}")
}

#[test]
fn mixed_shocks_keep_stochastic_and_scheduled_rows_separate() {
    let model = parse(&file(
        "shocks; var e; stderr .1; var d; periods 2 3; values .2 .3; end;",
    ));
    assert_eq!(model.shock_blocks.len(), 1);
    let block = &model.shock_blocks[0];
    assert_eq!(block.kind, ShockBlockKind::Regular);
    assert_eq!(block.stochastic.len(), 1);
    assert_eq!(block.scheduled.len(), 1);
    assert_eq!(model.shock_stmts.len(), 1);
    assert_eq!(model.name(block.scheduled[0].name), "d");
    assert_eq!(block.scheduled[0].periods.len(), 2);
    assert_eq!(block.scheduled[0].values[1].text, ".3");
    assert_eq!(block.scheduled[0].operation, ShockOperation::Values);
}

#[test]
fn surprise_multiplicative_and_heteroskedastic_forms_keep_options_and_rows() {
    let model = parse(&file(
        "shocks(surprise); var e; periods 2; values .1; end; \
         mshocks(relative_to_initval overwrite learnt_in=2); var e; periods 2; values 1.1; end; \
         heteroskedastic_shocks; var e; periods 1; values .1; var e; periods 2; scales 1.1; end;",
    ));
    assert_eq!(model.shock_blocks.len(), 3);
    assert_eq!(model.shock_blocks[0].kind, ShockBlockKind::Surprise);
    assert!(model.shock_blocks[0].stochastic.is_empty());
    assert_eq!(model.shock_blocks[1].kind, ShockBlockKind::Multiplicative);
    assert!(model.shock_blocks[1].options.relative_to_initval);
    assert!(model.shock_blocks[1].options.overwrite);
    assert!(matches!(
        model.shock_blocks[1].options.learnt_in,
        Some(PeriodPoint::Integer(2))
    ));
    assert_eq!(model.shock_blocks[2].kind, ShockBlockKind::Heteroskedastic);
    assert_eq!(model.shock_blocks[2].scheduled.len(), 2);
    assert_eq!(
        model.shock_blocks[2].scheduled[1].operation,
        ShockOperation::Scales
    );
    assert!(model.shock_stmts.is_empty());
}

#[test]
fn path_period_end_does_not_close_block_and_scoped_values_are_kept() {
    let model = parse(&file(
        "database db; shock_paths(learnt_in=2000Q1+1 overwrite); \
         var e; periods 1, 2:end; values db.x, exp(p)+self.e(-1); \
         exogenize y; periods 2; values initval.y+1; endogenize e; end;",
    ));
    assert_eq!(model.databases.len(), 1);
    assert_eq!(model.name(model.databases[0].names[0].0), "db");
    assert_eq!(model.shock_paths.len(), 1);
    let block = &model.shock_paths[0];
    assert_eq!(block.stanzas.len(), 2);
    assert!(block.options.overwrite);
    assert!(
        matches!(block.options.learnt_in, Some(PeriodPoint::Date(ref d)) if d.text == "2000Q1+1")
    );
    assert!(matches!(
        block.stanzas[0].periods[1].last,
        Some(PeriodPoint::End)
    ));
    assert_eq!(
        block.stanzas[0].values[0].path_refs[0].namespace.as_deref(),
        Some("db")
    );
    let refs = &block.stanzas[0].values[1].path_refs;
    assert_eq!(model.name(refs[1].name), "p");
    assert_eq!(refs[2].namespace.as_deref(), Some("self"));
    assert_eq!(refs[2].lag.as_deref(), Some("-1"));
    assert!(matches!(
        block.stanzas[1].target,
        PathTarget::Controlled { .. }
    ));
    assert!(model.namespace_qualified.is_empty());
}

#[test]
fn controlled_paths_endval_dates_subsamples_and_irf_options_are_structured() {
    let model = parse(&file(
        "perfect_foresight_controlled_paths(learnt_in=2); \
         exogenize y; periods 2; values (1); endogenize e; end; \
         endval(learnt_in=2000Q1); e += .1; y *= 1.1; end; \
         set_time(2000Q1+1+2); \
         p.subsamples(a=2000Q1:2001Q1,b=2001Q2+1:2002Q1); \
         p.subsamples = p.subsamples; \
         stoch_simul(irf=0,irf_shocks=(e));",
    ));
    assert_eq!(model.controlled_paths.len(), 1);
    assert_eq!(model.controlled_paths[0].stanzas.len(), 1);
    assert_eq!(model.endval_instructions.len(), 1);
    assert_eq!(model.endval_instructions[0].entries.len(), 2);
    assert_eq!(
        model.endval_instructions[0].entries[0].operation,
        ShockOperation::Add
    );
    assert_eq!(
        model.endval_instructions[0].entries[1].operation,
        ShockOperation::Multiply
    );
    assert_eq!(model.set_time[0].value.text, "2000Q1+1+2");
    assert_eq!(model.subsamples.len(), 2);
    assert!(
        matches!(&model.subsamples[0], SubsampleInstruction::Declare { ranges, .. } if ranges.len() == 2 && ranges[1].first.text == "2001Q2+1")
    );
    assert!(matches!(
        &model.subsamples[1],
        SubsampleInstruction::Copy { .. }
    ));
    assert_eq!(model.stoch_simul_requests[0].irf.unwrap().0, 0);
    assert_eq!(
        model.name(model.stoch_simul_requests[0].irf_shocks.as_ref().unwrap()[0].0),
        "e"
    );
}

#[test]
fn date_typed_command_options_keep_full_offsets() {
    let model = parse(&file(
        "data(file='x.csv',first_obs=2000Q1+1,last_obs=2001Q1); \
         initval_file(datafile='x.csv',first_obs=2000Q1+1,last_obs=2001Q1); \
         perfect_foresight_setup(first_simulation_period=2000Q1+2,last_simulation_period=2001Q4); \
         plot_shock_decomposition(plot_init_date=2000Q1+3,plot_end_date=2001Q1);",
    ));
    let values: Vec<_> = model
        .date_options
        .iter()
        .map(|opt| opt.value.text.as_str())
        .collect();
    assert_eq!(
        values,
        ["2000Q1+1", "2001Q1", "2000Q1+1", "2001Q1", "2000Q1+2", "2001Q4", "2000Q1+3", "2001Q1"]
    );
}

#[test]
fn subsample_named_prior_head_is_kept_for_later_lookup_checks() {
    let model = parse(&file(
        "p.subsamples(a=2000Q1:2001Q1); p.a.prior(shape=normal,mean=0,stdev=1);",
    ));
    assert_eq!(model.subsamples.len(), 1);
    let prior = model
        .dotted_statements
        .iter()
        .find(|stmt| stmt.kind == DottedKind::Prior)
        .unwrap();
    assert!(
        matches!(prior.head, DottedHead::Param { first, second: Some(range) }
        if model.name(first) == "p" && model.name(range) == "a")
    );
}

#[test]
fn std_and_corr_subsamples_declarations_and_copies_keep_heads() {
    let model = parse(&file(
        "std(e).subsamples(a=2000Q1:2001Q1); \
         corr(e,d).subsamples(b=2000Q1:2001Q1); \
         std(e).subsamples = p.subsamples;",
    ));
    assert_eq!(model.subsamples.len(), 3);
    assert!(matches!(
        &model.subsamples[0],
        SubsampleInstruction::Declare {
            head: dygnosis::model::SubsampleHead::Std(..),
            ..
        }
    ));
    assert!(matches!(
        &model.subsamples[1],
        SubsampleInstruction::Declare {
            head: dygnosis::model::SubsampleHead::Corr(..),
            ..
        }
    ));
    assert!(matches!(
        &model.subsamples[2],
        SubsampleInstruction::Copy {
            target: dygnosis::model::SubsampleHead::Std(..),
            source: dygnosis::model::SubsampleHead::Symbol(..),
            ..
        }
    ));
}

#[test]
fn named_prior_options_copies_and_repeated_subsample_statements_stay_parse_quiet() {
    let source = file(
        "p.a.prior = p.b.prior; p.a.options = p.b.options; \
         p.subsamples(a=2000Q1:2001Q1); p.subsamples(b=2001Q2:2002Q1);",
    );
    let model = parse(&source);
    assert_eq!(model.subsamples.len(), 2);
    assert!(!analyze(&model).iter().any(|d| d.code == "E001"));
}

#[test]
fn path_values_keep_expression_boundaries() {
    let source = "var y; varexo e; parameters p q r; p=.5; q=.2; r=.1; \
                  model; y=p*y(-1)+e; end; \
                  shock_paths; var e; periods 1,2; values p+q, r; end;";
    let model = parse(source);
    let values = &model.shock_paths[0].stanzas[0].values;
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].text, "p+q");
    assert_eq!(values[1].text, "r");
    assert!(!analyze(&model).iter().any(|d| d.code == "E001"));
    if let Some(binary) = find_preprocessor(None) {
        let result = run_preprocessor(
            source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            result.success,
            "7.2 refused separated path values: {} {}",
            result.raw_stdout, result.raw_stderr
        );
    }
}

#[test]
fn unclosed_shock_families_name_their_actual_opener() {
    for (body, keyword) in [
        ("mshocks; var e; periods 1; values 1.1;", "mshocks"),
        (
            "heteroskedastic_shocks; var e; periods 1; values .1;",
            "heteroskedastic_shocks",
        ),
    ] {
        let source = file(body);
        let expected = format!("Missing 'end;' for '{keyword}' block");
        assert!(
            analyze(&parse(&source))
                .iter()
                .any(|d| d.code == "E001" && d.message.starts_with(&expected)),
            "missing opener name: {source}"
        );
        if let Some(binary) = find_preprocessor(None) {
            let result = run_preprocessor(
                &source,
                &binary,
                None,
                Duration::from_secs(30),
                JsonStage::Check,
            );
            assert!(!result.success, "7.2 accepted unclosed block: {source}");
            let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
            assert!(
                output.contains("syntax error, unexpected end of file, expecting END or VAR"),
                "wrong 7.2 refusal: {output}"
            );
        }
    }
}

#[test]
fn pinned_72_accepts_new_forms_and_refuses_nearby_syntax() {
    let Some(binary) = find_preprocessor(None) else {
        return;
    };
    let accepted = [
        file("shocks; var e; stderr .1; var d; periods 2 3; values .2 .3; end;"),
        file("shocks(surprise); var e; periods 2; values .1; end;"),
        file("mshocks(relative_to_initval overwrite learnt_in=2); var e; periods 2; values 1.1; end;"),
        file("heteroskedastic_shocks; var e; periods 1; values .1; var e; periods 2; scales 1.1; end;"),
        file("database db; shock_paths; var e; periods 1, 2:end; values db.x, self.e(-1); end;"),
        file("perfect_foresight_controlled_paths(learnt_in=2); exogenize y; periods 2; values (1); endogenize e; end;"),
        file("p.subsamples(a=2000Q1:2001Q1); set_time(-2000Q1+1+2);"),
        file("p.subsamples(a=2000Q1:2001Q1); p.a.prior(shape=normal,mean=0,stdev=1);"),
        file("p.a.prior = p.b.prior; p.a.options = p.b.options; p.subsamples(a=2000Q1:2001Q1); p.subsamples(b=2001Q2:2002Q1);"),
        file("shocks(learnt_in=2000Q1+1); var e; periods 2000Q2; values .1; end;"),
        file("data(file='x.csv',first_obs=2000Q1+1,last_obs=2001Q1);"),
        file("initval_file(datafile='x.csv',first_obs=2000Q1+1,last_obs=2001Q1);"),
        file("perfect_foresight_setup(first_simulation_period=2000Q1+2,last_simulation_period=2001Q4);"),
        file("plot_shock_decomposition(plot_init_date=2000Q1+3,plot_end_date=2001Q1);"),
    ];
    for source in accepted {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            result.success,
            "7.2 rejected accepted shape: {} {}",
            result.raw_stdout, result.raw_stderr
        );
        assert!(
            !analyze(&parse(&source)).iter().any(|d| d.code == "E001"),
            "Dygnosis refused a 7.2 accepted shape: {source}"
        );
    }
    // One refused shape per row. The editor diagnostic must use the exact
    // 7.2 syntax sentence on these token-level parse refusals.
    let refused = [
        (
            file("shocks(surprise); var e; stderr .1; end;"),
            "syntax error, unexpected STDERR, expecting PERIODS",
            "stderr",
        ),
        (
            file("shock_paths; var e; periods 1 2; values 1,2; end;"),
            "syntax error, unexpected INT_NUMBER, expecting COMMA or ';'",
            "2",
        ),
        (file("p.subsamples();"), "syntax error, unexpected ')'", ")"),
        (file("set_time(+2000Q1);"), "syntax error, unexpected PLUS, expecting DATE", "+"),
        (file("set_time(- 2000Q1);"), "syntax error, unexpected MINUS, expecting DATE", "-"),
        (
            file("mshocks(overwrite,relative_to_initval); var e; periods 2; values 1.1; end;"),
            "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or RELATIVE_TO_INITVAL or ')'",
            ",",
        ),
        (
            file("shock_paths(overwrite,learnt_in=2); var e; periods 2; values 1; end;"),
            "syntax error, unexpected COMMA, expecting OVERWRITE or LEARNT_IN or ')'",
            ",",
        ),
        (
            file("shock_paths; exogenize y; periods 2:end; values 1; endogenize e; end;"),
            "syntax error, unexpected END, expecting INT_NUMBER",
            "end",
        ),
        (
            file("shocks; var e; periods 2; values p; end;"),
            "syntax error, unexpected IDENTIFIER",
            "p",
        ),
        (
            file("shock_paths; var e; periods 1,2; values p q; end;"),
            "syntax error, unexpected IDENTIFIER, expecting COMMA or ';'",
            "q",
        ),
    ];
    for (source, official_sentence, offending_text) in refused {
        let result = run_preprocessor(
            &source,
            &binary,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(!result.success, "7.2 accepted refused shape: {source}");
        let output = format!("{} {}", result.raw_stdout, result.raw_stderr);
        assert!(
            output.contains(official_sentence),
            "missing {official_sentence:?}: {output}"
        );
        let diagnostics = analyze(&parse(&source));
        let ours = diagnostics
            .iter()
            .find(|d| d.code == "E001" && d.message == official_sentence)
            .unwrap_or_else(|| panic!("missing exact E001 {official_sentence:?}: {diagnostics:?}"));
        assert_eq!(
            &source[ours.span.start as usize..ours.span.end as usize],
            offending_text
        );
    }
}
