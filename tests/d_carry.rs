use dygnosis::{analyze, parse};

fn errors(source: &str) -> Vec<(String, String)> {
    analyze(&parse(source))
        .into_iter()
        .filter(|d| d.severity == dygnosis::Severity::Error)
        .map(|d| (d.code, d.message))
        .collect()
}

#[test]
fn e278_follows_written_denominator_simplification() {
    for denominator in ["c-c", "y-y", "(c+y)-(y+c)", "(c+y)-y-c", "0*y"] {
        let source = format!("var y c; varexo e; model; y=1/({denominator})+e; c=y; end;");
        let got = errors(&source);
        assert!(
            got.iter().any(|(code, message)| code == "E278"
                && message == "Division by zero when forming (1)/(0); denominator simplified to 0 (possibly after substituting a variable set to 0)."),
            "{denominator}: {got:?}"
        );
    }
    let zero_numerator = errors("var y; varexo e; model; y=0/(y-y)+e; end;");
    assert!(zero_numerator.iter().any(|(code, message)| code == "E278"
        && message.starts_with("Division by zero when forming (0)/(0);")));

    for denominator in ["c-y", "a"] {
        let source = format!(
            "var y c; varexo e; parameters a; a=0; model; y=1/({denominator})+e; c=y; end;"
        );
        assert!(
            !errors(&source).iter().any(|(code, _)| code == "E278"),
            "{denominator}"
        );
    }
}

#[test]
fn command_lists_read_final_symbol_kinds() {
    for source in [
        "var y; varexo e; model_local_variable mv; model; y=e; end; forecast mv;",
        "var y; varexo e; model; y=e; end; steady_state_model; zz2=1; y=zz2; end; forecast zz2;",
        "var y c; varexo e; model; y=e; c=y; end; var_remove c; stoch_simul c;",
        "var y c; varexo e; model; y=e; c=y; end; stoch_simul c; var_remove c;",
    ] {
        let got = errors(source);
        assert!(
            got.iter()
                .any(|(code, message)| code == "E240"
                    && message.contains("is not one of {endogenous}")),
            "{source}: {got:?}"
        );
        assert!(
            !got.iter().any(|(code, _)| code == "E239"),
            "{source}: {got:?}"
        );
    }
}

#[test]
fn model_local_declaration_reaches_duplicate_codes() {
    let same = analyze(&parse(
        "var y; varexo e; model_local_variable mv; model_local_variable mv; model; y=e; end;",
    ));
    assert!(
        same.iter()
            .any(|d| d.code == "W031" && d.message == "Symbol mv declared twice."),
        "{same:?}"
    );
    let cross = errors("var y; varexo e; parameters mv; model_local_variable mv; model; y=e; end;");
    assert!(
        cross
            .iter()
            .any(|(c, m)| c == "E030" && m == "Symbol mv declared twice with different types!"),
        "{cross:?}"
    );
    let unknown_remove = errors("var y; varexo e; model; y=e; end; var_remove z;");
    assert!(
        unknown_remove
            .iter()
            .any(|(c, m)| c == "E058" && m == "Unknown symbol: z."),
        "{unknown_remove:?}"
    );
}

#[test]
fn carried_subsample_and_removed_use_sentences() {
    let base =
        "var y; varexo e u; varexo_det d; parameters a b; a=.5; b=.3; model; y=a*y(-1)+e+u; end;";
    for (suffix, code, sentence) in [
        (
            "a.subsamples(s=2000Q1:2000Q2,s=2001Q1:2001Q2);",
            "E427",
            "Symbol s may only be assigned once in a SUBSAMPLE statement",
        ),
        (
            "b.subsamples=a.subsamples;",
            "E428",
            "a does not have an associated subsample statement.",
        ),
        (
            "a.s.options(jscale=0.2);",
            "E429",
            "A subsample statement has not been issued for a",
        ),
        (
            "a.subsamples(s=2000Q1:2000Q2); a.t.options(init=0);",
            "E430",
            "The subsample name t was not previously declared in a subsample statement.",
        ),
        (
            "d.subsamples(s=2000Q1:2000Q2);",
            "E431",
            "subsamples: invalid symbol type for d",
        ),
        (
            "a.subsamples(s=2000Q1:2000Q2); d.subsamples=a.subsamples;",
            "E431",
            "subsamples: invalid symbol type for d",
        ),
    ] {
        let got = errors(&format!("{base} {suffix}"));
        assert!(
            got.iter().any(|(c, m)| c == code && m == sentence),
            "{suffix}: {got:?}"
        );
    }
    let removed = errors("var y c; varexo e; var_remove c; model; y=c+e; end;");
    assert!(removed.iter().any(|(c, m)| c == "E426" && m == "Variable 'c' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement"), "{removed:?}");
}

#[test]
fn options_correlation_type_check_has_its_own_sentence() {
    let base = "var y; varexo e u; model; y=e+u; end;";
    let mixed = errors(&format!("{base} corr(y,e).options(jscale=0.2);"));
    assert!(mixed.iter().any(|(c,m)| c == "E379" && m == "In the corr(A,B).options statement, A and B must be of the same type. In your case, y and e are of different types."), "{mixed:?}");
    let same = errors(&format!("{base} corr(e,u).options(jscale=0.2);"));
    assert!(!same.iter().any(|(c, _)| c == "E379"), "{same:?}");
    let copy = errors(&format!("{base} corr(y,e).options=corr(e,u).options;"));
    assert!(!copy.iter().any(|(c, _)| c == "E379"), "{copy:?}");
}

#[test]
fn named_copy_sources_keep_type_checks_without_subsample_lookup() {
    let base =
        "var y; varexo e u; varexo_det d; parameters a b; a=.5; b=.2; model; y=a*y(-1)+b+e+u; end;";
    for (statement, code, message) in [
        ("a.s.options=y.s.options;", "E378", "y is not a parameter"),
        ("a.s.options=z.s.options;", "E058", "Unknown symbol: z."),
        (
            "std(e).s.options=std(d).s.options;",
            "E317",
            "d is an exogenous deterministic.",
        ),
        (
            "corr(e,u).s.options=corr(e,a).s.options;",
            "E059",
            "a is neither endogenous or exogenous.",
        ),
        ("a.s.prior=y.s.prior;", "E378", "y is not a parameter"),
        (
            "std(e).s.prior=std(d).s.prior;",
            "E317",
            "d is an exogenous deterministic.",
        ),
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            got.iter().any(|(c, m)| c == code && m == message),
            "{statement}: {got:?}"
        );
    }
    for statement in ["a.s.options=b.s.options;", "a.s.prior=b.s.prior;"] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            !got.iter()
                .any(|(c, _)| ["E428", "E429", "E430"].contains(&c.as_str())),
            "{statement}: {got:?}"
        );
    }
}

#[test]
fn named_std_corr_heads_use_declarations_available_at_the_statement() {
    let base = "var y; varexo e; model; y=e; end;";
    for statement in [
        "std(u).s.options(jscale=0.2); varexo u;",
        "corr(e,u).s.options(jscale=0.2); varexo u;",
        "std(u).s.prior(shape=normal,mean=0,stdev=1); varexo u;",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            got.iter()
                .any(|(code, message)| code == "E058" && message.contains("u")),
            "{statement}: {got:?}"
        );
        assert!(
            !got.iter().any(|(code, _)| code == "E429"),
            "{statement}: {got:?}"
        );
    }
    let declared_first = errors("var y; varexo e u; model; y=e+u; end; std(u).subsamples(s=2000Q1:2000Q2); std(u).s.options(jscale=0.2);");
    assert!(
        !declared_first.iter().any(|(code, _)| code == "E058"),
        "{declared_first:?}"
    );
    let native_plain =
        errors("var y; varexo e; model; y=e; end; a.s.options(init=0); parameters a;");
    assert!(
        !native_plain
            .iter()
            .any(|(code, _)| ["E058", "E429"].contains(&code.as_str())),
        "{native_plain:?}"
    );
}

#[test]
fn handed_command_shapes_are_parse_errors_not_unknown_names() {
    let base = "var y; varexo e; model; y=0.5*y(-1)+e; end;";
    for statement in [
        "forecast nograph;",
        "stoch_simul nograph;",
        "calib_smoother nograph;",
        "forecast conf_sig;",
        "forecast();",
        "forecast(y);",
        "forecast y(1);",
        "forecast zzz=1;",
        "rplot;",
        "rplot(y);",
        "rplot(zzz=1) y;",
        "rplot nograph;",
        "rplot periods;",
        "dynasave y;",
        "dynatype y;",
        "dynasave('f.csv',nobs=4) y;",
        "dynasave('f.csv') nograph;",
        "dynasave('f.csv') datafile;",
        "shock_decomposition();",
        "shock_decomposition(y);",
        "shock_decomposition(colormap);",
        "shock_decomposition nograph;",
        "realtime_shock_decomposition(y);",
        "initial_condition_decomposition(y);",
        "initial_condition_decomposition type;",
        "plot_shock_decomposition(y);",
        "plot_shock_decomposition(with_epilogue) y;",
        "plot_shock_decomposition detail_plot;",
        "plot_shock_decomposition periods;",
        "squeeze_shock_decomposition(y);",
        "squeeze_shock_decomposition();",
        "squeeze_shock_decomposition nograph;",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(got.iter().any(|(c, _)| c == "E001"), "{statement}: {got:?}");
        assert!(
            !got.iter().any(|(c, _)| c == "E239"),
            "{statement}: {got:?}"
        );
    }
    for statement in [
        "forecast(nograph) y;",
        "forecast(periods=8) y;",
        "rplot y;",
        "rplot e;",
        "dynasave('f.csv') y;",
        "dynatype('f.m') y;",
        "squeeze_shock_decomposition y;",
        "plot_shock_decomposition(fig_name='a') y;",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            !got.iter().any(|(c, _)| c == "E001"),
            "{statement}: {got:?}"
        );
    }
    let alpha = errors("var y alpha; varexo e; model; y=e; alpha=y; end; forecast alpha;");
    assert!(
        !alpha
            .iter()
            .any(|(c, _)| ["E001", "E239", "E240"].contains(&c.as_str())),
        "{alpha:?}"
    );
}

#[test]
fn subsample_reach_and_quiet_neighbours() {
    let base =
        "var y; varexo e u; varexo_det d; parameters a b; a=.5; b=.3; model; y=a*y(-1)+e+u; end;";
    for (suffix, code) in [
        ("std(z).subsamples(s=2000Q1:2000Q2);", "E058"),
        ("a.subsamples=z.subsamples;", "E058"),
        ("y.s.options(init=0);", "E378"),
        ("std(d).s.options(init=0);", "E317"),
        ("std(a).s.options(init=0);", "E059"),
        ("a.subsamples(s=2000Q1:4);", "E001"),
        ("a.subsamples(s=4:2000Q1);", "E001"),
        ("a.options(overwrite);", "E001"),
    ] {
        let got = errors(&format!("{base} {suffix}"));
        assert!(got.iter().any(|(c, _)| c == code), "{suffix}: {got:?}");
    }
    for suffix in [
        "z.subsamples(s=2000Q1:2000Q2);",
        "z.subsamples=a.subsamples;",
        "a.s.prior=b.s.prior;",
        "a.s.options=b.s.options;",
        "a.subsamples(s=2000Q1:2000Q2); a.subsamples(s=2001Q1:2001Q2);",
        "corr(e,u).subsamples(s=2000Q1:2000Q2); corr(u,e).s.options(jscale=0.2);",
        "corr(e,d).subsamples(s=2000Q1:2000Q2);",
        "d.subsamples(s=2000Q1:2000Q2); change_type(parameters) d;",
    ] {
        let got = errors(&format!("{base} {suffix}"));
        assert!(
            !got.iter()
                .any(|(c, _)| ["E427", "E428", "E429", "E430", "E431"].contains(&c.as_str())),
            "{suffix}: {got:?}"
        );
    }
    let earlier_refusal = errors(&format!(
        "{base} a.subsamples(s=2000Q1:2000Q2); change_type(varexo_det) a;"
    ));
    assert!(
        earlier_refusal.iter().any(|(c, _)| c == "E296"),
        "{earlier_refusal:?}"
    );
    assert!(
        !earlier_refusal.iter().any(|(c, _)| c == "E431"),
        "{earlier_refusal:?}"
    );
}

#[test]
fn date_consumers_keep_72_accept_and_refuse_boundary() {
    let base = "var y; varexo e; parameters a; a=.5; model; y=a*y(-1)+e; end;";
    for statement in [
        "set_time(2000Q1+1);",
        "data(file='x.csv',first_obs=2000Q1+1,last_obs=2001Q1);",
        "histval_file(datafile='x.csv',first_obs=4,last_obs=5);",
        "initval_file(datafile='x.csv',first_obs=2000Q1+1,last_obs=2001Q1,first_simulation_period=2000Q2,last_simulation_period=2001Q1);",
        "perfect_foresight_setup(first_simulation_period=2000Q1+1,last_simulation_period=2001Q1);",
        "perfect_foresight_with_expectation_errors_setup(first_simulation_period=2000Q1+1,last_simulation_period=2001Q1);",
        "initial_condition_decomposition(plot_init_date=2000Q1+1,plot_end_date=2001Q1) y;",
        "plot_shock_decomposition(plot_init_date=2000Q1+1,plot_end_date=2001Q1) y;",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(!got.iter().any(|(c,_)| c == "E001"), "{statement}: {got:?}");
    }
    for statement in [
        "set_time(4);",
        "set_time(2000Q1-1);",
        "data(file='x.csv',first_obs=4);",
        "perfect_foresight_setup(first_simulation_period=4);",
        "perfect_foresight_with_expectation_errors_setup(last_simulation_period=4);",
        "initial_condition_decomposition(plot_init_date=4) y;",
        "plot_shock_decomposition(plot_end_date=4) y;",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(got.iter().any(|(c, _)| c == "E001"), "{statement}: {got:?}");
    }
}

#[test]
fn date_consumers_refuse_invalid_suffix_after_valid_prefix() {
    let base = "var y; varexo e; parameters a; a=.5; model; y=a*y(-1)+e; end;";
    for (statement, message) in [
        (
            "data(file='x.csv',first_obs=2000Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "histval_file(first_obs=2000Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "initval_file(first_obs=2000Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "perfect_foresight_setup(first_simulation_period=2000Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "perfect_foresight_with_expectation_errors_setup(last_simulation_period=2000Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "plot_shock_decomposition(plot_init_date=2000Q1-1) y;",
            "syntax error, unexpected MINUS, expecting ')'",
        ),
        (
            "initial_condition_decomposition(plot_end_date=2000Q1-1) y;",
            "syntax error, unexpected MINUS, expecting ')'",
        ),
        (
            "set_time(2000Q1+1.5);",
            "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER",
        ),
        (
            "a.subsamples(s=2000Q1:2001Q1-1);",
            "syntax error, unexpected MINUS, expecting COMMA or ')'",
        ),
        (
            "a.subsamples(s=2000Q1:2001Q1+1.5);",
            "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER",
        ),
        (
            "data(file='x.csv',first_obs=2000Q1+1.5);",
            "syntax error, unexpected FLOAT_NUMBER, expecting INT_NUMBER",
        ),
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            got.iter()
                .any(|(code, text)| code == "E001" && text == message),
            "{statement}: {got:?}"
        );
    }
    for statement in [
        "data(file='x.csv',first_obs=2000Q1+1);",
        "histval_file(first_obs=4,last_obs=2000Q1+1);",
        "initval_file(first_obs=4,last_obs=2000Q1+1);",
        "perfect_foresight_setup(first_simulation_period=2000Q1+1);",
        "plot_shock_decomposition(plot_init_date=2000Q1+1) y;",
        "a.subsamples(s=2000Q1:2001Q1+1);",
    ] {
        let got = errors(&format!("{base} {statement}"));
        assert!(
            !got.iter().any(|(code, _)| code == "E001"),
            "{statement}: {got:?}"
        );
    }
}
