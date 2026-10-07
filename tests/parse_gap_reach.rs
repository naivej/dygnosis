//! Slice 20 reach audit: every row is replayed at its catalog catching step.

use std::{collections::HashMap, time::Duration};

use dygnosis::server::diagnostics_for;
use dygnosis::span::LineIndex;
use dygnosis::{
    analyze, dynare_diagnose, find_preprocessor, parse, run_preprocessor, JsonStage, Severity,
};
use tower_lsp::lsp_types::{DiagnosticSeverity, NumberOrString};

const BASE: &str = "var y; varexo e u; varexo_det d; parameters rho beta; rho=.8; beta=.5;\nmodel; #x=1; y=rho*y(-1)+beta+e+u+d+x; end;\n";
const PLAIN: &str = "var y; varexo e u; varexo_det d; parameters rho beta; rho=.8; beta=.5;\nmodel; y=rho*y(-1)+beta+e+u+d; end;\n";

struct Reach {
    code: &'static str,
    tail: &'static str,
    needle: &'static str,
    stage: JsonStage,
}

fn row(code: &'static str, tail: &'static str, needle: &'static str) -> Reach {
    Reach {
        code,
        tail,
        needle,
        stage: JsonStage::Check,
    }
}

fn native_rows() -> Vec<Reach> {
    vec![
        row("E426", "var z; var_remove z; rho=z;", "Variable 'z' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement"),
        row("E090", "varobs e;", "e is not endogenous."),
        row("E095", "var z; varobs z; observation_trends; y(1); end;", "is not an observed variable"),
        row("E130", "var z; steady_state_model; z=y; y=0; end;", "variable 'y' is undefined in the declaration of variable 'z'"),
        row("E295", "change_type(parameters) zzz;", "Unknown variable zzz"),
        row("E311", "filter_initial_state; rho(0)=.9; end;", "filter_initial_state: rho should be an endogenous or exogenous variable"),
        row("E318", "ramsey_model; ramsey_constraints; y>0; y<1; end;", "two constraints for variable y"),
        row("E332", "homotopy_setup; y,0,1; end;", "homotopy_val: y should be a parameter or exogenous variable"),
        row("E361", "svar_identification; exclusion lag 0; equation 1,y,y; end;", "y restriction added twice."),
        row("E379", "corr(y,e).prior(shape=normal,mean=0,stdev=1);", "A and B must be of the same type. In your case, y and e"),
        row("E386", "matched_moments; e; end;", "Matched moment expression has incorrect format: Variable e is not an endogenous"),
        row("E388", "matched_irfs; var y; varexo e; periods 1; values 1; var y; varexo e; periods 2; values 2; end;", "matched_irfs: the pair endogenous y with exogenous e appears two times"),
        row("E389", "matched_irfs_weights; y(1),e,y(2),e,.5; y(1),e,y(2),e,.7; end;", "matched_irfs: the tuple (y(1),e,y(2),e) appears two times"),
        row("W206", "deterministic_trends; rho(1); end;", "Warning: Non-variable symbol used in deterministic_trends: rho"),
        Reach { code: "W205", tail: "shock_groups; g1=e; g1=u; end;", needle: "shock group label 'g1' has been reused", stage: JsonStage::Write },
        row("E065", "model; y=steady_state(e); end;", "Exogenous variables are not allowed in the context of the STEADY_STATE() operator."),
        row("E182", "occbin_constraints; name 'ELB'; bind e<=0; end;", "Exogenous variable e cannot be used in 'occbin_constraints'."),
        row("E251", "planner_objective e;", "You cannot include exogenous variables"),
        row("E253", "model; #xx=1; y=xx; end; planner_objective xx;", "Model local variable xx cannot be used in 'planner_objective'."),
        row("E263", "model; [mcp='e>0'] y=e; end;", "Left-hand side of expression in 'mcp' tag is not an endogenous variable"),
        row("E279", "external_function(name=myf); rho=myf;", "name of a MATLAB/Octave function"),
        row("E280", "external_function(name=myf); model; y=myf; end;", "function name external to Dynare"),
        row("E281", "rho=ghost; model; y=ghost; end;", "Variable ghost not allowed inside model declaration. Its scope is only outside model."),
        row("E282", "model; #xx=1; y=xx; end; rho=xx;", "not allowed outside model declaration. Its scope is only inside model"),
        row("E294", "epilogue; zz=y; end; rho=zz;", "cannot be used outside the epilogue block"),
        row("E464", "heterogeneity_dimension hh; var(heterogeneity=hh) a; epilogue; z=a; end;", "Symbol 'a' cannot be used in epilogue block, because it is heterogeneous."),
        row("E465", "heterogeneity_dimension hh; var(heterogeneity=hh) a; varexo(heterogeneity=hh) eh; parameters(heterogeneity=hh) ph; model(heterogeneity=hh); a=a(-1)+eh+ph; end; shocks(heterogeneity=hh); var y=1; end;", "setting a variance on 'y' is not allowed, because it is not a heterogeneous exogenous variable"),
        row("E466", "heterogeneity_dimension hh; var(heterogeneity=hh) a; varexo(heterogeneity=hh) eh; parameters(heterogeneity=hh) ph; model(heterogeneity=hh); a=a(-1)+eh+ph; end; shocks(heterogeneity=hh); var y; stderr 1; end;", "setting a standard error on 'y' is not allowed, because it is not a heterogeneous exogenous variable"),
        row("E467", "heterogeneity_dimension hh; var(heterogeneity=hh) a; varexo(heterogeneity=hh) eh; parameters(heterogeneity=hh) ph; model(heterogeneity=hh); a=a(-1)+eh+ph; end; shocks(heterogeneity=hh); var y,e=1; end;", "setting a covariance between 'y' and 'e'is not allowed"),
        row("E468", "heterogeneity_dimension hh; var(heterogeneity=hh) a; varexo(heterogeneity=hh) eh; parameters(heterogeneity=hh) ph; model(heterogeneity=hh); a=a(-1)+eh+ph; end; shocks(heterogeneity=hh); corr y,e=1; end;", "setting a correlation between 'y' and 'e'is not allowed"),
        Reach { code: "E433", tail: "var_model(model_name=v,eqtags=['Missing']);", needle: "looking for equation tag Missing failed.", stage: JsonStage::Transform },
        Reach { code: "E435", tail: "var_expectation_model(model_name=a,variable=y,auxiliary_model_name=missing,horizon=1);", needle: "var_expectation_model a refers to nonexistent auxiliary model missing", stage: JsonStage::Transform },
        row("E440", "pac_model(model_name=q,discount=rho); pac_model(model_name=q,discount=rho);", "a PAC model already exists with the name q"),
        row("E442", "var_expectation_model(model_name=a,variable=y,auxiliary_model_name=v,horizon=1,discount=y);", "The discount factor must be a constant expression or a parameter"),
        row("E444", "pac_model(model_name=q,discount=y);", "y is not a parameter"),
        Reach { code: "E446", tail: "pac_model(model_name=q,discount=rho,auxiliary_model_name=missing);", needle: "aux_model_name not recognized as VAR model or Trend Component model", stage: JsonStage::Transform },
        Reach { code: "E447", tail: "var z; model; z=var_expectation(nope); end;", needle: "unknown model 'nope' used in var_expectation expression", stage: JsonStage::Transform },
        Reach { code: "E451", tail: "var z; model; z=pac_expectation(nope); end;", needle: "pac_expectation operator references an unknown pac_model", stage: JsonStage::Transform },
        Reach { code: "E452", tail: "var z; model; z=pac_target_nonstationary(nope); end;", needle: "pac_target_nonstationary operator does not match a corresponding 'pac_target_info' block", stage: JsonStage::Transform },
        row("E419", "endval(learnt_in=2); y=1; end;", "endval(learnt_in=...): y is not an exogenous variable"),
        row("E111", "heterogeneity_dimension hh; varexo(heterogeneity=hh) eh; shocks(heterogeneity=hh); var eh=.1; var eh=.2; end;", "shocks: variance or stderr of shock on eh declared twice"),
        row("E393", "heterogeneity_dimension hh; varexo(heterogeneity=hh) eh; shocks(heterogeneity=hh); skew eh=.1; skew eh=.2; end;", "shocks: skewness of eh declared twice"),
        row("E394", "heterogeneity_dimension hh; varexo(heterogeneity=hh) eh fh gh; shocks(heterogeneity=hh); skew eh,fh,gh=.1; skew gh,eh,fh=.2; end;", "shocks: co-skewness of (gh, eh, fh) declared twice"),
        row("E023", "predetermined_variables e;", "e is not endogenous."),
        row("E184", "occbin_constraints; name 'ELB'; bind y<=0; bind y<=1; end;", "The 'bind' clause is declared multiple times"),
        row("E260", "varexobs y;", "varexobs: y is not an exogenous variable"),
        row("E274", "generate_irfs; a,e=1,e=2; end;", "You have set the exogenous variable e twice."),
        row("E307", "trend_var(growth_factor=1.02) A; trend_var(growth_factor=1.02) A;", "Trend variable A was declared twice."),
        row("E308", "trend_var(growth_factor=1.02) A; var(deflator=A) y,y;", "Variable y was listed more than once as following a trend."),
        row("E309", "var z w; trend_var(growth_factor=1.02) A; var(deflator=A) z; var(deflator=z) w;", "The deflator contains a non-stationary endogenous variable."),
        row("E310", "trend_var(growth_factor=1.02) A; rho=A;", "Variable A not allowed outside model declaration, because it is a trend variable."),
        row("E316", "var z; optim_weights; y,z 1; y,z 2; end;", "optim_weights: pair of variables (y, z) declared twice"),
        row("E394", "varexo v; shocks; skew e,u,v=1; skew v,e,u=2; end;", "shocks: co-skewness of (v, e, u) declared twice"),
        row("E460", "heterogeneity_dimension hh; heterogeneity_dimension hh;", "Heterogeneity dimension 'hh' already declared"),
        row("E461", "heterogeneity_dimension hh; var(heterogeneity=hh) a; planner_objective a;", "Symbol 'a' cannot be used in 'planner_objective', because it is heterogeneous."),
        row("E462", "heterogeneity_dimension hh; var(heterogeneity=hh) a; occbin_constraints; name 'ELB'; bind a>0; end;", "Symbol 'a' cannot be used in 'occbin_constraints', because it is heterogeneous."),
        row("E463", "heterogeneity_dimension hh; var(heterogeneity=hh) a; rho=a;", "Symbol 'a' cannot be used outside model declaration, because it is heterogeneous."),
        row("E245", "estimated_params; stderr e; stderr e; end;", "the stderr of e is declared twice."),
        row("E246", "estimated_params; corr e,u,.1; corr e,u,.2; end;", "the correlation between e and u is declared twice."),
        row("E247", "estimated_params; skew e,0; skew e,.1; end;", "the skewness of e is declared twice."),
        row("E335", "model_remove('nosuchtag');", "were not found."),
        row("E337", "model; [name='a'] y=e; [name='b'] y=u; end; model_remove('a','b');", "Variable y was excluded twice via a model_remove or model_replace statement"),
        row("E256", "model; [name='a',name='b'] y=e; end;", "Tag 'name' cannot be used twice for the same equation"),
        row("E478", "model; y=SUM(e); end;", "The argument to the SUM() operator must be a heterogeneous endogenous variable"),
        row("E275", "rho=pp.rho;", "Namespace-qualified symbol pp.rho not allowed in this context"),
        Reach { code: "E186", tail: "var z;", needle: "z not used in the model block", stage: JsonStage::Transform },
        row("E030", "varexo y;", "declared twice with different types"),
        row("W031", "parameters rho;", "Symbol rho declared twice"),
        row("E058", "initval; nope=1; end;", "Unknown symbol: nope."),
        row("E059", "std(rho).options(init=0);", "rho is neither endogenous or exogenous."),
        row("E317", "std(d).options(init=0);", "d is an exogenous deterministic."),
        row("E271", "rho.prior(shape=normal,mean=0,mean=1,stdev=1);", "option mean declared twice"),
        row("E378", "y.prior(shape=normal,mean=0,stdev=1);", "y is not a parameter"),
        row("E427", "rho.subsamples(s=2000Q1:2000Q2,s=2001Q1:2001Q2);", "Symbol s may only be assigned once in a SUBSAMPLE statement"),
        row("E428", "rho.subsamples=beta.subsamples;", "beta does not have an associated subsample statement."),
        row("E429", "rho.s.options(init=0);", "A subsample statement has not been issued for rho"),
        row("E430", "rho.subsamples(s=2000Q1:2000Q2); rho.t.options(init=0);", "The subsample name t was not previously declared in a subsample statement."),
        Reach { code: "E431", tail: "d.subsamples(s=2000Q1:2000Q2);", needle: "subsamples: invalid symbol type for d", stage: JsonStage::Write },
        row("E020", "model; y=nope+e+u+d; end;", "Unknown symbol: nope"),
        row("E093", "estimated_params; nope, normal_pdf, 0, 1; end;", "Unknown symbol: nope"),
        row("E111", "shocks; var e=1; var e=2; end;", "variance or stderr of shock on e declared twice"),
        row("E266", "shocks; var rho=1; end;", "setting a variance on 'rho' is not allowed"),
        row("E239", "stoch_simul nope;", "Variable nope was not declared"),
        row("E240", "stoch_simul rho;", "is not one of"),
        row("W202", "stoch_simul y y;", "y found more than once in symbol list"),
        row("E001", "rho=\"hello\";", "character unrecognized by lexer"),
        row("E101", "ramsey_model(instruments=(nope));", "Unknown symbol: nope"),
        row("E317", "ramsey_model(instruments=(rho));", "rho is not endogenous."),
        row("E025", "model; #rho=1; y=e+u+d; end;", "rho has wrong type or was already used on the right-hand side"),
        row("E030", "model; #x=1; #x=2; y=e+u+d+x; end;", "Local model variable x declared twice"),
        row("E243", "histval; y(0)=1; y(0)=2; end;", "histval: y(0) declared twice"),
        row("E313", "filter_initial_state; y(0)=1; y(0)=2; end;", "filter_initial_state: (y, 0) declared twice"),
        row("E315", "optim_weights; y 1; y 2; end;", "optim_weights: y declared twice"),
        row("E258", "varobs y; varobs y;", "varobs: you cannot have several 'varobs' statements in the same MOD file"),
        row("E259", "varexobs e; varexobs e;", "varexobs: you cannot have several 'varexobs' statements in the same MOD file"),
        row("E261", "varobs y; observation_trends; y(1); y(2); end;", "observation_trends: y declared twice"),
        row("E387", "mshocks; var rho; periods 1; values 1; end;", "rho is not exogenous."),
        row("E344", "shocks; var e; periods 1; values 1; var e; periods 2; values 2; end;", "shocks/conditional_forecast_paths: variable e declared twice"),
        row("E402", "heteroskedastic_shocks; var e; periods 1; values 1; var e; periods 2; values 2; end;", "heteroskedastic_shocks: variable e declared twice"),
        row("E407", "shock_paths; var e; periods 1; values y; end;", "In the shock_paths block, parameters are the only symbols allowed without a namespace-qualifier"),
        row("E415", "shock_paths; var e; periods 1; values db.x; end;", "Unknown database: db."),
        row("E414", "database db; database db;", "Database 'db' already declared"),
        row("E244", "estimated_params; rho; rho; end;", "rho is declared twice"),
        row("E329", "init2shocks; y e; y e; end;", "appears more than once"),
        row("E330", "init2shocks; rho e; end;", "rho should be an endogenous variable"),
        row("E331", "init2shocks; y rho; end;", "rho should be an exogenous variable"),
        row("E333", "shock_groups; group=y; end;", "shock_groups: y should be an exogenous variable"),
        row("E288", "epilogue; z=nope; end;", "Variable nope used in the epilogue block but was not declared."),
        row("E393", "shocks; skew e=1; skew e=2; end;", "shocks: skewness of e declared twice"),
        row("E459", "model(heterogeneity=ghost); y=1; end;", "Unknown heterogeneity dimension: ghost"),
        row("E481", "steady_state_model; e=1; end;", "e has incorrect type"),
        row("E287", "epilogue; z=y; z=y+1; end;", "variable 'z' is declared twice"),
        row("E289", "epilogue; z=e; end;", "Symbol 'e' cannot be used inside the epilogue block, because it is an exogenous variable."),
        row("E290", "epilogue; z=d; end;", "Symbol 'd' cannot be used inside the epilogue block, because it is an exogenous deterministic variable."),
        row("E267", "shocks; var rho; stderr 1; end;", "setting a standard error on 'rho' is not allowed"),
        row("E268", "shocks; var y,e=1; end;", "setting a covariance between 'y' and 'e'is not allowed"),
        row("E269", "shocks; corr y,e=1; end;", "setting a correlation between 'y' and 'e'is not allowed"),
        row("E270", "shocks; skew y=0; end;", "skewness can only be specified for exogenous variables"),
        row("E249", "estimated_params; skew y,0; end;", "skewness can only be specified for exogenous variables, not for 'y'"),
        row("W131", "steady_state_model; y=0; y=1; end;", "variable 'y' is declared twice"),
    ]
}

fn local_rows() -> Vec<Reach> {
    vec![
        row("E426", "var z; var_remove z; x=z;", "Variable 'z' can no longer be used since it has been excluded by a previous 'model_remove' or 'var_remove' statement"),
        row(
            "E282",
            "x=x;",
            "not allowed outside model declaration. Its scope is only inside model",
        ),
        row(
            "E279",
            "external_function(name=myf); x=myf;",
            "name of a MATLAB/Octave function",
        ),
        row(
            "E294",
            "epilogue; zz=y; end; x=zz;",
            "cannot be used outside the epilogue block",
        ),
        row(
            "E310",
            "trend_var(growth_factor=1.02) A; x=A;",
            "because it is a trend variable",
        ),
        row(
            "E463",
            "heterogeneity_dimension hh; var(heterogeneity=hh) a; x=a;",
            "cannot be used outside model declaration, because it is heterogeneous",
        ),
        row(
            "E001",
            "x(1);",
            "syntax error, unexpected '(', expecting EQUAL or '.'",
        ),
        row("E378", "x=1;", "x is not a parameter"),
        row(
            "E275",
            "x=pp.rho;",
            "Namespace-qualified symbol pp.rho not allowed in this context",
        ),
        row(
            "E378",
            "x.prior(shape=normal,mean=0,stdev=1);",
            "x is not a parameter",
        ),
        row("E378", "x.options(init=0);", "x is not a parameter"),
        row(
            "E378",
            "x.s.prior(shape=normal,mean=0,stdev=1);",
            "x is not a parameter",
        ),
        row("E378", "x.s.options(init=0);", "x is not a parameter"),
        row("E378", "rho.prior=x.prior;", "x is not a parameter"),
        row("E378", "rho.options=x.options;", "x is not a parameter"),
        row("E378", "rho.s.prior=x.s.prior;", "x is not a parameter"),
        row("E378", "rho.s.options=x.s.options;", "x is not a parameter"),
        row("E378", "x.prior=rho.prior;", "x is not a parameter"),
        row("E378", "x.options=rho.options;", "x is not a parameter"),
        row(
            "E378",
            "[x, rho].prior(shape=normal,mean=[0,0],variance=[[1,0],[0,1]]);",
            "x is not a parameter",
        ),
        row(
            "E271",
            "x.prior(shape=normal,mean=0,mean=1,stdev=1);",
            "option mean declared twice",
        ),
        row(
            "E271",
            "x.options(init=0,init=1);",
            "option init declared twice",
        ),
        row(
            "E059",
            "std(x).prior(shape=normal,mean=0,stdev=1);",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E059",
            "std(x).options(init=0);",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E059",
            "corr(x,e).prior(shape=normal,mean=0,stdev=1);",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E059",
            "corr(x,e).options(init=0);",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E059",
            "std(e).options=std(x).options;",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E059",
            "std(e).prior=std(x).prior;",
            "x is neither endogenous or exogenous.",
        ),
        row(
            "E058",
            "x.subsamples=nope.subsamples;",
            "Unknown symbol: nope.",
        ),
        row(
            "E427",
            "x.subsamples(s=2000Q1:2000Q2,s=2001Q1:2001Q2);",
            "Symbol s may only be assigned once in a SUBSAMPLE statement",
        ),
        row(
            "E428",
            "x.subsamples=beta.subsamples;",
            "beta does not have an associated subsample statement.",
        ),
        Reach {
            code: "E431",
            tail: "x.subsamples(s=2000Q1:2000Q2);",
            needle: "subsamples: invalid symbol type for x",
            stage: JsonStage::Write,
        },
        Reach {
            code: "E431",
            tail: "rho.subsamples(s=2000Q1:2000Q2); x.subsamples=rho.subsamples;",
            needle: "subsamples: invalid symbol type for x",
            stage: JsonStage::Write,
        },
        Reach {
            code: "E431",
            tail: "std(x).subsamples(s=2000Q1:2000Q2);",
            needle: "subsamples: invalid symbol type for x",
            stage: JsonStage::Write,
        },
    ]
}

fn has_code(source: &str, code: &str) -> bool {
    analyze(&parse(source)).iter().any(|d| d.code == code)
}

/// Check both adapters against the library, including both written range edges.
fn transports(source: &str, code: &str, needle: Option<&str>) {
    transports_in(source, None, None, code, needle);
}

fn transports_in(
    source: &str,
    path: Option<&str>,
    files: Option<&HashMap<String, String>>,
    code: &str,
    needle: Option<&str>,
) {
    let library = path.map_or_else(
        || analyze(&parse(source)),
        |path| dygnosis::check_file(source, path),
    );
    let own: Vec<_> = library.iter().filter(|d| d.code == code).collect();
    let mcp = dynare_diagnose(source, path, files);
    let mcp: Vec<_> = mcp.iter().filter(|d| d.code == code).collect();
    let lsp = diagnostics_for(path.unwrap_or("file:///C:/tmp/parse_gap_reach.mod"), source);
    let lsp: Vec<_> = lsp
        .iter()
        .filter(|d| matches!(&d.code, Some(NumberOrString::String(found)) if found == code))
        .collect();
    assert_eq!(mcp.len(), own.len(), "{source}: {mcp:?} versus {own:?}");
    assert_eq!(lsp.len(), own.len(), "{source}: {lsp:?} versus {own:?}");
    if let Some(needle) = needle {
        assert!(
            own.iter().any(|d| d.message.contains(needle)),
            "{source}: expected {code}: {needle}, got {library:?}"
        );
    } else {
        assert!(own.is_empty(), "{source}: unexpected {code}: {own:?}");
    }
    let index = LineIndex::new(source);
    for ((own, mcp), lsp) in own.iter().zip(mcp).zip(lsp) {
        assert_eq!(mcp.message, own.message);
        assert_eq!(lsp.message, own.message);
        let (mcp_severity, lsp_severity) = match own.severity {
            Severity::Error => ("ERROR", DiagnosticSeverity::ERROR),
            Severity::Warning => ("WARNING", DiagnosticSeverity::WARNING),
            Severity::Information => ("INFORMATION", DiagnosticSeverity::INFORMATION),
            Severity::Hint => ("HINT", DiagnosticSeverity::HINT),
        };
        assert_eq!(mcp.severity, mcp_severity);
        assert_eq!(lsp.severity, Some(lsp_severity));
        let start = index.position(source, own.span.start);
        let end = index.position(source, own.span.end);
        assert_eq!(
            (mcp.line, mcp.column, mcp.end_line, mcp.end_column),
            (
                start.line + 1,
                start.character + 1,
                end.line + 1,
                end.character + 1
            )
        );
        let start = index.position_utf16(source, own.span.start);
        let end = index.position_utf16(source, own.span.end);
        assert_eq!(
            (
                lsp.range.start.line,
                lsp.range.start.character,
                lsp.range.end.line,
                lsp.range.end.character
            ),
            (start.line, start.character, end.line, end.character)
        );
    }
}

#[test]
fn native_remainders_remove_file_relative_unknown_and_wrong_type_readers() {
    let pp = find_preprocessor(None);
    for (fixture, sidecar, tail, code, sentence, stage) in [
        (
            "d_open/w204_load_params_unknown.mod",
            "w204_params.txt",
            "load_params_and_steady_state('w204_params.txt');",
            "W204",
            "Unknown symbol zzz in w204_params.txt",
            JsonStage::Check,
        ),
        (
            "d_writer/e380_load_params_epilogue.mod",
            "e380_params.txt",
            "epilogue; A=1; end; load_params_and_steady_state('e380_params.txt');",
            "E380",
            "Unsupported variable type for A in load_params_and_steady_state",
            JsonStage::Write,
        ),
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let path_text = path.to_str().unwrap();
        let dir = path.parent().unwrap();
        let sidecar_path = dir.join(sidecar);
        let data = std::fs::read_to_string(&sidecar_path).unwrap();
        for native in [false, true] {
            let source = format!("{PLAIN}{}{tail}\n", if native { "plot(1); " } else { "" });
            let files = HashMap::from([
                (path_text.to_string(), source.clone()),
                (sidecar_path.to_str().unwrap().to_string(), data.clone()),
            ]);
            transports_in(
                &source,
                Some(path_text),
                Some(&files),
                code,
                (!native).then_some(sentence),
            );
            assert_eq!(parse(&source).load_params_file.is_none(), native);
            if let Some(pp) = &pp {
                let result =
                    run_preprocessor(&source, pp, Some(dir), Duration::from_secs(30), stage);
                let text = format!("{}{}", result.raw_stdout, result.raw_stderr);
                if native {
                    assert!(
                        result.success && !text.contains(sentence),
                        "{source}: {text}"
                    );
                } else {
                    assert!(text.contains(sentence), "{source}: {text}");
                }
            }
        }
    }
}

#[test]
fn native_remainders_remove_each_generic_reader_and_keep_its_real_entry() {
    let baseline = parse(PLAIN);
    for row in native_rows() {
        let native = format!("{PLAIN}plot(1); {}\n", row.tail);
        transports(&native, row.code, None);
        assert!(
            !analyze(&parse(&native))
                .iter()
                .any(|d| d.severity == Severity::Error),
            "{native}"
        );
        let model = parse(&native);
        assert_eq!(
            model.endogenous.len(),
            baseline.endogenous.len(),
            "{native}"
        );
        assert_eq!(model.exogenous.len(), baseline.exogenous.len(), "{native}");
        assert_eq!(
            model.parameters.len(),
            baseline.parameters.len(),
            "{native}"
        );
        assert_eq!(model.equations.len(), baseline.equations.len(), "{native}");
        assert_eq!(
            model.param_assignments.len(),
            baseline.param_assignments.len(),
            "{native}"
        );
        assert!(model.dotted_statements.is_empty(), "{native}");
        assert!(model.shocks_block.is_none(), "{native}");
        assert!(model.initval_block.is_none(), "{native}");
        let real = format!("{PLAIN}{}\n", row.tail);
        // Existing editor wording warrants remain in force for these readers.
        // The official needle is checked independently by the honesty replay.
        transports(&real, row.code, Some(""));
    }
    let repeated_shape = format!("{BASE}plot(1); rho.prior(shape=beta,shape=beta,mean=.5);\n");
    transports(&repeated_shape, "E271", None);
    assert!(parse(&repeated_shape).dotted_statements.is_empty());
}

#[test]
fn both_model_local_declaration_forms_reach_dotted_vector_and_copy_readers() {
    for base in [
        BASE.to_string(),
        BASE.replacen("model;", "model_local_variable x; model;", 1),
    ] {
        for row in local_rows() {
            transports(&base, row.code, None);
            let source = format!("{base}{}\n", row.tail);
            transports(&source, row.code, Some(row.needle));
            if row.code != "E058" {
                assert!(!has_code(&source, "E058"), "{source}");
            }
            if row.code == "E275" {
                assert!(!has_code(&source, "E378"), "{source}");
            }
        }
    }
    for tail in [
        "rho.prior(shape=normal,mean=0,stdev=1);",
        "rho.options(init=0);",
        "[rho, beta].prior(shape=normal,mean=[0,0],variance=[[1,0],[0,1]]);",
        "rho.subsamples(s=2000Q1:2000Q2); rho.s.options(init=0);",
    ] {
        transports(&format!("{BASE}{tail}\n"), "E378", None);
    }
}

#[test]
fn syntax_recovery_has_matching_fire_and_quiet_on_both_transports() {
    for (source, needle) in [
        (
            format!("{BASE}rho=1 ...\n+0;\n"),
            "syntax error, unexpected '.'",
        ),
        (
            "var y; varexo e; model; y={1}; end;\n".into(),
            "character unrecognized by lexer",
        ),
        (
            "var y; varexo e; model; y=y';\nend;\n".into(),
            "character unrecognized by lexer",
        ),
        (
            "var y; model; y=1; end\nshocks; var e; end;\n".into(),
            "syntax error, unexpected SHOCKS, expecting ';'",
        ),
        (
            format!("{BASE}shocks; var e; end;\n"),
            "syntax error, unexpected END, expecting PERIODS",
        ),
    ] {
        transports(&source, "E001", Some(needle));
    }
    for source in [
        format!("{BASE}plot(1 ...\n); rho=pp.rho;\n"),
        format!("{BASE}{{1,2}}; rho=pp.rho;\n"),
        format!("{BASE}\"hello\";\n"),
        format!("{BASE}shocks; var e; stderr 1; end;\n"),
        "var y; model; y=1; end\nplot(1);\n;\n".into(),
    ] {
        transports(&source, "E001", None);
    }
    for source in [
        "parameters rho;\n@#define dots=\"...\"\nrho=1 @{dots}\n+0;\n",
        "var y; varexo e;\n@#define bad=\"{1}\"\nmodel; y=@{bad}; end;\n",
    ] {
        transports(source, "E001", Some(""));
    }
}

#[test]
fn repaired_syntax_refusals_and_controls_are_honest() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping slice 20 syntax honesty: Dynare 7.2 is absent");
        return;
    };
    for (source, sentence) in [
        (
            format!("{BASE}rho=1 ...\n+0;\n"),
            "syntax error, unexpected '.'",
        ),
        (
            format!("{BASE}rho=1...\n+0;\n"),
            "syntax error, unexpected '.'",
        ),
        (
            "var y; varexo e; model; y={1}; end;\n".into(),
            "character unrecognized by lexer",
        ),
        (
            "var y; varexo e; model; y=y';\nend;\n".into(),
            "character unrecognized by lexer",
        ),
        (
            "var y; model; y=1; end\nshocks; var e; end;\n".into(),
            "syntax error, unexpected SHOCKS, expecting ';'",
        ),
        (
            "var y; model; y=1; end".into(),
            "syntax error, unexpected end of file, expecting ';'",
        ),
        (
            format!("{BASE}plot(\"hello); rho=pp.rho;\n"),
            "character unrecognized by lexer",
        ),
    ] {
        let result = run_preprocessor(
            &source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        let text = format!("{}{}", result.raw_stdout, result.raw_stderr);
        assert!(
            !result.success && text.contains(sentence),
            "{source}: {text}"
        );
        transports(&source, "E001", Some(sentence));
    }
    for source in [
        format!("{BASE}plot(1 ...\n); rho=pp.rho;\n"),
        format!("{BASE}{{rho}}; rho=pp.rho;\n"),
        format!("{BASE}@rho; rho=pp.rho;\n"),
        format!("{BASE}[rho,\tbeta].prior(shape=beta,mean=.5); rho=pp.rho;\n"),
        format!("{BASE}\"hello\";\n"),
        "var y; model; y=1; end\nplot(1);\n;\n".into(),
        "var y(long_name='a\n{b}'); model; y=1; end;\n".into(),
    ] {
        let result = run_preprocessor(
            &source,
            &pp,
            None,
            Duration::from_secs(30),
            JsonStage::Check,
        );
        assert!(
            result.success,
            "{source}: {}{}",
            result.raw_stdout, result.raw_stderr
        );
        transports(&source, "E001", None);
        transports(&source, "E275", None);
    }
}

#[test]
fn reach_is_honest_at_parse_check_transform_and_write() {
    let Some(pp) = find_preprocessor(None) else {
        eprintln!("skipping slice 20 honesty: Dynare 7.2 is absent");
        return;
    };
    let mut failures = Vec::new();
    for row in native_rows() {
        let native = format!("{PLAIN}plot(1); {}\n", row.tail);
        let result = run_preprocessor(&native, &pp, None, Duration::from_secs(30), row.stage);
        if !result.success || has_code(&native, row.code) {
            failures.push(format!(
                "native {}: {native}: {}{}",
                row.code, result.raw_stdout, result.raw_stderr
            ));
        }
        let real = format!("{PLAIN}{}\n", row.tail);
        let result = run_preprocessor(&real, &pp, None, Duration::from_secs(30), row.stage);
        let text = format!("{}{}", result.raw_stdout, result.raw_stderr);
        if !text.contains(row.needle) || !has_code(&real, row.code) {
            failures.push(format!(
                "real {}: {real}: expected {}, got {text}; ours {:?}",
                row.code,
                row.needle,
                analyze(&parse(&real))
            ));
        }
    }
    for base in [
        BASE.to_string(),
        BASE.replacen("model;", "model_local_variable x; model;", 1),
    ] {
        for row in local_rows() {
            let quiet = run_preprocessor(&base, &pp, None, Duration::from_secs(30), row.stage);
            if !quiet.success || has_code(&base, row.code) {
                failures.push(format!(
                    "local quiet {}: {}{}",
                    row.code, quiet.raw_stdout, quiet.raw_stderr
                ));
            }
            let source = format!("{base}{}\n", row.tail);
            let result = run_preprocessor(&source, &pp, None, Duration::from_secs(30), row.stage);
            let text = format!("{}{}", result.raw_stdout, result.raw_stderr);
            if result.success || !text.contains(row.needle) || !has_code(&source, row.code) {
                failures.push(format!(
                    "local {}: {source}: expected {}, got {text}",
                    row.code, row.needle
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
