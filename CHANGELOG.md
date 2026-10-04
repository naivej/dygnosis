# Changelog

## v0.11.6

- The Windows x64 release executable is about 9% smaller than in 0.11.5. The Windows x64 standalone archive is about 6% smaller, and the VSIX is about 5% smaller. The editor and agent transports both stay in the binary.
- W011, "Parameter assignment cannot be evaluated", is reported in written source order. Macro assignments that share one source range keep a stable order.

## v0.11.5

- Model-expression diagnostics reach heterogeneous and replacement equations, epilogue expressions, planner and Ramsey expressions, OccBin constraints, trends and deflators, matched moments, VAR expectations, PAC expressions, and complementarity conditions.
- External-function calls in those expressions require a declaration before use and the declared argument count. Calls with integer arguments remain function calls. Invalid variable calls and malformed argument lists use Dynare's messages and point to the written call or the offending token.
- Symbol-role errors and deterministic-exogenous timing errors use Dynare's wording on these expressions. A later declaration or type change does not change an earlier use.

## v0.11.4

- Duplicate checks reach heterogeneous shock rows for E111, E393, and E394, and link to the earlier row.
- Includes in inactive macro branches and empty loops no longer produce false missing-file or syntax errors.
- Steady-state assignments accept valid scalar targets and multiple outputs. A target of the wrong type is E481, `NAME has incorrect type`, using the type at that assignment. The editor marks the output name. An endogenous or temporary name used on the right side before it is assigned is E130.
- Valid assigned outputs, excluded names, and qualified native calls no longer receive the repaired false diagnostics. Right-side symbol roles use Dynare's sentences.
- Related-file discovery retains qualified external-function and derivative-helper names and resolves their package files.
- Model expressions retain complete qualified external-function calls, require their declarations at the call, and check argument counts and malformed arguments with Dynare's sentences.

## v0.11.3

- Saved `.mod` files in a workspace folder are checked when they are not open. `dynare.projectDiagnostics` is on by default. Progress is separate from the model-count bar. **Dygnosis: Recheck project** runs discovery again. Cancelling pauses that pass until an edit or Recheck.
- `dynare.projectExcludePaths` keeps matching roots out of that discovery. An open model keeps its editor diagnostics. An excluded file that another model includes still affects that model. `.dyn`, loose, and untitled models keep their editor diagnostics. Turning project checks off leaves ordinary diagnostics on open files.
- The extension page shows the shipped editor, structural Diff, and effective-model jumps.

## v0.11.2

- **Diff with…** compares the active model (Before) with a chosen model (After). It shows symbols, parameter values, aggregate equations, heterogeneous equations, and shock setup, with change and dimension filters. Each side opens its written source when that location is known. Edits, includes, and search-path changes mark the comparison stale until refresh.
- Those written locations are also in MCP comparison results. Values and pairing stay the same.
- The effective-model preview jumps to the written equation and to the macro directive that produced the row, including the loop value. **Refresh effective model** replaces the text and those jumps together. Edits and changed inputs disable jumps until refresh. The preview stays read-only.
- **Dygnosis: Set up MCP for this project** writes project-local `.mcp.json` in a trusted workspace. The bundled engine is copied to a stable path. Managed updates keep that command path. On Windows, a running agent process keeps the current executable until it stops. Other servers in the file, and global agent settings, stay as they are.

## v0.11.1

First VS Code extension. It checks open models and the files they include.

- Adds a VS Code extension for `.mod`, `.dyn`, and `.inc`, for VS Code 1.102 or newer. The extension includes the engine for Windows, macOS, and Linux, on x64 and ARM64. `dynare.serverPath` selects another executable for the editor and for VS Code's agent tools. Those tools register before a model is opened.
- Adds model counts, a Dynare model view, and equation navigation. An include asks which model owns it when more than one model is known. Block tinting follows the theme and can be set per block. **Browse equations** CodeLens is on. Declaration references and **Show effective model** CodeLens stay off until enabled.
- **Ignore this check** hides one code in this window. **Show** and **Show all** restore hidden codes. The file, the command line, and agents still report those codes. **Explain this check** opens a read-only explanation. Existing fixes keep their diagnostic and source context.
- **Show effective model** opens a read-only preview of the expanded model. That preview stays outside model checking.
- The language server applies document opens and edits before it answers a later request. A request that is waiting can still be cancelled.
- Prepares a VSIX and a standalone archive for each of the six targets above, with the engine, licenses, and checksums. Publication remains open.

## v0.11.0

Language server and agent tools.

- Workspace folders and loose files keep separate include paths and cached inputs. A settings change sends the full configuration. Clients can opt in to `dynare/modelInfoChanged` when the model should refresh.
- Adds `dynare/modelInfo`. Counts and timing match `dynare_model_info`. The response also has written statements, declarations, counted equations, block categories, related files, and source segments. Outline and folding stay in the file on screen. Incomplete expansion withholds counts and equation numbers. Equation numbers describe the written model before Dynare rewrites it.
- Hover and completion show the written `long_name` and TeX name. Each can be hidden. Completion uses separate icons for endogenous variables, exogenous variables, and parameters, and offers empty block skeletons. Highlights mark declarations and assignment targets as writes, and other uses as reads.
- Semantic tokens negotiate `dynareEndogenous`, `dynareExogenous`, `dynareParameter`, and `dynareModelLocal`. A client that does not support a role receives `variable`. Full and range responses use that legend. Existing Dynare-specific color rules need the [selector migration](docs/editor-settings.md#semantic-token-migration).
- Duplicate and include-cycle diagnostics link to the earlier written location in the editor and in MCP results. A fix in an unopened include uses the current source. An edit to an open file uses that file's version. W020 and W022 carry LSP Unnecessary. E021 does not.
- Expression-value hints return for an assignment whose right side is not a plain number. The hint is the value at that assignment when the arithmetic is finite and known. Plain numbers stay quiet. Unknown execution, and macro copies that do not share one value, withhold the hint. Hover values and comparison values stay as they were.

## v0.10.2

- Internal symbol history uses distinct types for symbol kinds and parser positions. Explicit removal-fallback queries preserve existing diagnostic and view behavior.

- Subsample definitions, copies, and named prior/options uses follow macro execution order. An earlier written line in a later iteration can use a definition already executed in the previous iteration without false E428/E429.

- Extract retains successful type changes for selected names in parser order. Space-separated declaration lists filter unselected names while preserving TeX and per-name options.
- Finishes the written-parameter-list audit. Initial/history values, filter rows, shock/path targets, init2shocks, homotopy, shock groups, varobs, estimated-parameter roles, Markov parameter options, and prior copy sources use the type at the statement. Macro iterations keep their own parser context for ordinary names too.
- Steady-state order, estimated skewness, parameters in shock values, deterministic-trend warnings, and PAC/VAR matching use final types. Completion, semantic colors, and compare names, kinds, and parameter values agree with the final-type views.

- Named external_function declarations reject unknown options with E001. Repeated valid options keep the first value, as Dynare does, without false E271.
- An unused trend_var or log_trend_var name can change to an ordinary type without false E295. Final counts and symbol lists include it while retaining its written trend declaration.
- Names excluded before optim_weights report E317; later excluded planner or Ramsey-constraint references report E426. Earlier accepted uses keep their removal warrants.
- The policy-generated discount parameter is known to later command lists, assignments, and duplicate checks. Its implicit creation keeps written declarations and counts intact.
- Non-model expression calls register unknown functions and distinguish integer arguments from variable timing. A later declaration or command list uses their known function kind; valid calls no longer get E279/E280 as bare function-name uses.
- `dsge_prior_weight` is reserved in Dynare blocks. Accepted parameter-assignment and policy-option expressions no longer get E001 for that name.
- Command symbol lists use final types after `change_type`, including exclusion/restoration and a heterogeneous name made ordinary. E240 no longer rejects valid retyped names or accepts a heterogeneous name as an ordinary type.
- A later declaration that clashes with an expression-created native local reports E030 with Dynare's declared-twice sentence.
- Command symbol lists report E240 for native locals introduced by expressions, including a list written before the expression. Native MATLAB assignment heads remain undeclared.
- Equation symbol classes, timing, model counts, and hover follow each name's final type after `change_type`. Declaration locations still point at the written declaration. Hover distinguishes `varexo_det` from ordinary `varexo`.
- `E251`, `E182`, and `E255` use a name's type after `change_type`, not the list where it was declared. A `varexo` retyped to a parameter is no longer an exogenous in the planner objective or in `occbin_constraints`, and a parameter retyped to `varexo` is. A `var` retyped to a parameter can be used in `osr_params_bounds`; a parameter retyped to `var` cannot.
- Policy instruments use their type when the policy command is read. Undeclared names use `E101`; declared names of the wrong type use `E317` with `N is not endogenous.` A later declaration or type change cannot validate an earlier instrument. Repeated macro iterations retain their own type checks.
- `discretionary_policy` reports `E378` when an existing `optimal_policy_discount_factor` is not a parameter at the command. That earlier refusal withholds instrument errors on the same command. Native locals introduced by preceding expressions or by the command's options use the declared wrong-type instrument sentence.
- A comment that mentions `differentiate_forward_vars` no longer hides the equation-count error. The option still withholds `E188` when it is written on `model(…)` or `model_options(…)`, because that option can add helper variables.

- Diagnostic location mapping reuses joined include text and source maps. Document edits, disk reloads, and search-path changes invalidate that snapshot while preserving included-file diagnostic locations.

## v0.10.1

- Macro functions, scalar `@#for ... when ...` filters, and tuple loops expand in diagnostics and model views, with loop origins kept. A filtered-out iteration no longer invents an unknown name or a count warning. A range bound can use arithmetic: `1:N-1` ends at `N-1`. An undefined macro variable or function is `E063` at the directive that uses it, and a quoted macro string used as a name is reported without its quotes. A valid expression Dygnosis cannot evaluate shows `I211` Information, withholds the checks that need the expanded model, and marks model info, equations, compare, and the effective-model view as incomplete.
- Equation counts use each name's final type after `change_type`. A `var` retyped to a parameter or `varexo` no longer counts for `W013`, `E186`, or `E188`, and a parameter or `varexo` retyped to `var` does.
- Parameter checks follow `change_type`. `z = 1;` or `z.prior(…)` after `change_type(parameters) z;` is no longer `E378`; before it, or after `change_type(var)`, it still is. `W010`, `W022`, and `W121` use the final type, so a parameter retyped to `var` no longer gets them and a `var` retyped to a parameter does.
- `E193` reports three PAC target-equation refusals visible in the written file: a nonlinear right side, a variable absent from `pac_target_info` components, or a listed component absent from the equation. That check runs before the missing-operator refusal. Unary, diff, and other rewrite-dependent forms remain quiet, as does a competing assignment that might be simplified first.
- A written constant division by zero is `E189` on the first proven left- or right-hand side. A direct unused endogenous is `E186`. A plain aggregate or heterogeneous equation count that cannot create helper variables is `E188` or `E192`. A direct two-variable `EXPECTATION(0)` sum under `partial_information` is `E190`. Counts and substitutions that depend on rewrite stay quiet.
- `W186` reports Dynare's auxiliary-name warning on a symbol list and stops that list. A duplicate name in the same list still warns.
- `W212` warns when an `initval` or `endval` assignment, including `endval(learnt_in=1)`, names a variable that a later `model_remove` or `var_remove` still excludes. `change_type` after `var_remove` restores the name, so the warning stays quiet and an unused restored exogenous is `E021`.
- An incomplete macro expansion withholds later workspace file and companion checks. The incomplete notice stays; a complete file still reports a real missing include or unknown value-file name.
- Command option lists follow the Dynare 7.2 grammar for membership, separators, and simple numeric values. A missing comma after an option value is reported on the next option, where Dynare reports it. `estimated_params_remove` parses its rows and checks each name when that row is read.
- Diagnostics and references from an included file use the included source. Batch diagnosis uses that same ownership.

## v0.10.0

- A non-ASCII character in active Dynare syntax is `E001` with Dynare's `character unrecognized by lexer`. That includes a declaration (`var café;`), an equation (`y = café;`), and a shock name. Unicode comments, `long_name`, TeX names, equation-name strings, verbatim text, native MATLAB text, and a discarded `@#if` branch stay accepted, as does the complementarity operator.
- A heterogeneity dimension whose written equations do not match its distinct written endogenous names warns `W208`, the same kind of count as aggregate `W013`. Leads and lags that Dynare later turns into helper variables are not part of this count. A repeated name counts once. An unresolved include or macro expansion withholds the count.
- `W211` warns when an ordinary aggregate `varexo` is written with a lead in an aggregate equation or model-local definition and the file contains `stoch_simul` or `estimation`. The lead is legal; the warning asks for a timing review. `varexo_det`, heterogeneous variables, lags, `simul`, and any `perfect_foresight_*` command stay quiet. There is no switch that turns the warning off.
- `x@{i}` in a macro loop is the identifier `x1`, `x2`, and so on, in diagnostics and model views.
- Comment silencing (`// dygnosis:disable` and the `vsd:` names) is removed. The editor, the command line, and agents all show the same diagnostics. Hiding a check in one editor, without writing it into the file, is a 0.11 editor action.
- Information codes `I208`, `I209`, and `I210` summarize missing equation `name` tags, missing declaration `long_name`s, and numeric literals written in model equations. Each note is one count for the compilation unit and points to its first affected source file, including through CLI, MCP and editor include handling. They are writing preferences, not Dynare refusals. An unresolved include or macro expansion withholds the count.
- When that equation-name summary is shown, the editor can add `eq_N` tags to the counted equations it can edit safely. Existing tags stay. A generated `@#for` copy, an equation whose source is ambiguous, and an include that would name more than one equation are left unchanged. The action title counts those skips. Running it again does not change the file.
- Compare pairs aggregate equations by name first. A unique name pairs across reordering; a repeated name pairs only when the normalized text and tags match. Equation rows include each side's name and tags, and `unmatched_same_name` groups leftovers that still share a name.
- Compare diffs each heterogeneity dimension on its own, with the same name and tag pairing. Those rows are `heterogeneous_equations`. Aggregate equation lists stay aggregate-only.
- Removes parameter-value inlay hints and the model-counts code lens. Folded assignment values stay on hover and in compare. Counts stay in `dynare_model_info`.
- `dynare_format` formats a `.mod` file with the same rules as the editor. It returns the full text when the formatting changes. Empty or whitespace-only input, and a file that is already formatted, are `unchanged`. Formatting that cannot be done safely is `unsupported`. `formatIndent` is `tab` or 1–8 spaces, as in the editor.
- `dynare_extract` returns a named equation group with the declarations, model locals, and heterogeneity dimension it needs. It retains required static/dynamic partners, OccBin constraint declarations, expanded equation order and macro iteration origins, including static rows. Supplied file maps are self-contained and preserve source-key identity. The text is a fragment, not a runnable or square model. An empty selector is an input error. No match is `empty`. Missing required setup, a PAC or VAR expectation, heterogeneous static replacement or OccBin regime context, or an unresolved include or macro returns `unsupported_context` with no fragment.
- `dynare_workspace_diagnose` checks several root `.mod` files in one call. Pass a files map with the roots to check, or paths to files and directories. Map includes, inherited include search paths and companion existence use only supplied files; unprovided disk files cannot affect the report. Disk paths with redundant `.` components are diagnosed once. Each root is reported on its own, and one failure does not drop the others.
- Compare reports `symbols_changed` for a shared name whose declaration kind, `long_name`, or TeX name differs, including metadata that was added or removed. Missing metadata stays empty rather than being filled with the symbol name. Parameter value changes stay in `changed_parameter_values`. A kind change across `var`, `varexo`, and `parameters` also stays in the existing added and removed name lists. A change between `varexo` and `varexo_det`, or between an aggregate declaration and the same command with a heterogeneity dimension, stays on that command's list and is reported in `symbols_changed`.
- Signature help inside a known command's option list shows that command's catalog option names and descriptions and marks the option being typed. The editor asks again on `(`, `,`, and `=`. A comma inside parentheses, brackets, braces, a string, or a comment stays on the same option. `shocks(heterogeneity=…)` lists the heterogeneous shock options. A model equation, and a command with no catalog options, has no signature.
- The editor offers named stochastic and deterministic shocks templates for eligible aggregate exogenous declarations. Clear stochastic or perfect-foresight contexts choose the appropriate form; ambiguous or mixed contexts offer both eligible forms. Each unfinished row and the wrapper are commented, and no standard deviation, period or value is filled in. Insertion stays outside every block, including unfinished blocks. A heterogeneous shocks block does not hide the actions, and nothing is applied automatically.
- LSP requests, diagnostics and edits use UTF-16 positions, including supplementary Unicode characters. MCP retains its existing scalar coordinates. Public equation rows retain `text` and omit the redundant `lhs` and `rhs` fields.

## v0.9.0

- Reads Dynare 7.2 heterogeneity dimensions, declarations, model blocks, shock rows, and the four heterogeneity commands while keeping aggregate and heterogeneous equations separate.
- `dynare_model_info` shows aggregate counts and per-dimension heterogeneous counts; `dynare_equations` returns heterogeneous rows with dimension and source origin. Variable hover and outline use timing from heterogeneous equations.
- `dynare_expand` and the editor's effective-model view include heterogeneous equations in their origin lists. The expand count covers all counted written model equations and reports aggregate and per-dimension counts separately.
- Reports the written heterogeneity refusals (`E459`–`E479` and the recorded `E001` shapes) and warns on a second distinct heterogeneity dimension (`W207`). The per-dimension equation count after auxiliary equations stays silent (`E192`).
- Adds Dynare 7.2 names and descriptions for the heterogeneity commands and options to completion, hover, and `dynare_list_options`. `SUM` is operator help. `heterogeneity` is listed on `shocks`. Hover and completion on `overwrite` inside `shocks(heterogeneity=…)` describe that dimension's variance settings; ordinary `shocks(overwrite)` keeps the regular-shocks sentence.
- `shock_setup_changes` pairs heterogeneous variance, standard-error, covariance, and correlation rows by dimension, and shows the written settings, overwrite status, and source locations.

## v0.8.0

- Reads Dynare 7.2 `var_model`, `trend_component_model`, `var_expectation_model`, `pac_model`, and `pac_target_info`, plus their named model operators. The independent `deterministic_trends` block is also checked. Parse summaries and equation tools still show the equations you wrote.
- Reports the family's parse and PAC target checks, selected VAR/trend equation shape and tag errors, VAR and PAC expectation name errors, and written PAC growth, target, operator-use, and fixed generated-name clashes (`E432`–`E458`, `W206`). Shared `E021` and `E058` now reach applicable family fields. Diagnostics use the same core in LSP and MCP.
- Adds Dynare 7.2 option names and descriptions to completion, hover, and `dynare_list_options`. These include bare `structural` and `eqtags`; `pac_target_info` body rows remain block content rather than command options.
- Checks that require Dynare's generated equations, substituted expressions, or lag-derived auxiliary names remain silent until their source causes can be mapped. The installed 7.2 PAC example passes the check-stage smoke test; it prints an unused-parameter warning.

## v0.7.0

- Reads the written shock, path, learning-date, and database forms accepted by Dynare 7.2 and reports their parse, check, and written transform refusals (`E393`–`E425`). Existing `E343`/`E344` now reach deterministic shocks; `E111` catches covariance followed by correlation and skew/co-skew duplicates have their own codes.
- Subsample declarations, copies, and named `prior` / `options` uses now report missing or repeated ranges (`E427`–`E430`) and invalid final target types at Write (`E431`). A model use after `var_remove` reports `E426`; command lists recognize removed and model-local names through `E240`. Written denominator cancellation reaches `E278`, and malformed DATE and handed-over command forms reach `E001`. Mixed-type `corr(A,B).options` uses `E379`.
- W060 names missing selected shocks from `irf_shocks`, or warns for an unselected positive `irf` only when no plain exogenous shock has a written size. It stays quiet for bare declarations, bare `stoch_simul`, explicit zero size, an estimated standard error for the plain exogenous shock, a possible external size, and uncertain macro or include input. An observed-variable measurement-error estimate alone does not suppress it. Warnings in a file with resolved includes point to the active file.
- Compare adds `shock_setup_changes` and a **Shock setup changes** markdown section for stochastic, scheduled, surprise, multiplicative, heteroskedastic, controlled, and terminal instructions. It shows written values, timing, learning dates, controls, and overwrite status, with source locations when the input text can be verified against the parsed file. It does not infer realized paths or solver results.
- Compare also shows changed initial and terminal values, time settings, database references, and heteroskedastic data settings beside affected shocks; `shock_groups` and `init2shocks` changes have a separate **Shock analysis setup** section.

## v0.6.1

Pins language checks and optional honesty tests to Dynare 7.2.

- **E243** uses 7.2's `histval: y(0) declared twice` sentence.
- `histval_file` no longer offers `nobs` or `last_simulation_period`; either option reports Dynare's syntax error on its name. `initval_file` still accepts both.
- Duplicate checks for shocks, histval, generated IRFs, filter initial state, observation trends, optimization weights, and estimated parameters compare entries within one block. Repeating an entry in a later block stays quiet. **E248** now checks a value reference only against parameters declared in the same block. Existing same-block refusals remain.

## v0.6.0

Reads method of moments, matched moments and IRFs, and the IRF and moment calibration blocks, and reports the refusals Dynare makes on them. Codes `E382`–`E392`.

Diagnostics implemented in this release:
- **E382**–**E385** — `method_of_moments` needs `mom_method` (`GMM`, `SMM`, or `IRF_MATCHING`); GMM and SMM need `datafile`; `analytic_standard_errors` and `analytic_jacobian` need GMM; only one of the HP, one-sided HP, and bandpass filters.
- **E386** — a `matched_moments` row is not a product of endogenous variables: `Matched moment expression has incorrect format`.
- **E387**–**E392** — a matched-IRF shock that is not exogenous, a repeated endogenous/exogenous pair or tuple, `periods` and `values` (or `weights`) of different lengths, and a date written in `periods`.
- **E058** and **E317** reach the name slots of `matched_irfs`, `matched_irfs_weights`, `irf_calibration`, and `moment_calibration`.
- **E239** and **E240** reach the trailing name lists of `forecast`, `rplot`, `dynasave`, `dynatype`, and the shock-decomposition commands. `rplot` allows an endogenous or an exogenous.

Other changes:
- `dynare_list_options` knows `matched_moments`, `matched_irfs`, `matched_irfs_weights`, and `moment_calibration`.
- A variable may be named after a block opener (`shocks`, `matched_irfs`) or after a command whose only lexer rule is at the start of a statement (`steady`, `dynatype`). `forecast`, `identification`, `simul`, `stoch_simul`, and `varobs` are still not names.
- `model = 0.2;` and `steady = 0.9;` at the start of a statement are syntax errors, as they are in Dynare. Inside a block, `end` is the block closer, not a name.

## v0.5.5

Adds the refusals the Dynare preprocessor prints only at its last stage, when it writes the MATLAB files — after its check and transform stages have already accepted the file. The file you edit is enough to decide each of them. Codes `E380`, `E381` and `W205`.

Diagnostics implemented in this release:
- **E380** — a `load_params_and_steady_state` file names something the loader cannot take: an `epilogue` helper, an `external_function` name (the `name=` value or a value named by `first_deriv_provided` / `second_deriv_provided`), or a trend variable. The four slots it accepts are parameter, endogenous, `varexo` and `varexo_det`. Dynare refuses only when writing: `Unsupported variable type for A in load_params_and_steady_state`.
- **E381** — a `steady_state(…)` expression calls an `external_function`: `The expression inside a steady_state operator cannot contain external functions`. The operand walk descends, so a call nested under an operator is caught too.
- **W205** — two rows of the same `shock_groups` block reuse a label: `shock group label 'g1' has been reused. Only using the last definition.` The comparison is within one block, as Dynare's is: two separate `shock_groups` blocks may share a label silently.

Other changes:
- **`W204` narrows**: the unsupported kinds above now error `E380` instead, and `W204` keeps the genuinely unknown name. All three kinds are positional, as Dynare's own reading is — a name declared only *after* the `load_params_and_steady_state` statement is still unknown to the loader, warns `Unknown symbol`, and stays `W204`.
- **Writer-stage honesty**: the test harness can now spawn a run that reaches the MATLAB writer (bare `nopreprocessoroutput`, without `onlyjson`, which is what skips the writer), and cleans the `+<name>/` package directory such a run leaves beside the `.mod`.
- The two writer-stage messages that no `.mod` shape can be matched against — the more-than-32-nested-parentheses warning (whose trigger is Dynare's generated text) and the excluded-name-still-in-`initval` refusal (which aborts Dynare with no message) — are documented as deliberately silent (`W187`, `E191`).

## v0.5.4

Adds the MS-SBVAR family: the `ms_*` commands, `sbvar`, `svar`, `markov_switching`, the `svar_identification` and `conditional_forecast_paths` blocks, `conditional_forecast`, `plot_conditional_forecast`, the `data` statement and the dotted `prior` statement are read instead of skipped, and the refuses Dynare makes on them are reported. Codes `E338`–`E379`.

Diagnostics implemented in this release:
- **The family's own refusals** (one code per distinct Dynare message): the `data` statement's file-or-series rule, its both-at-once rule and its `nobs` bound (`E338`–`E340`); `ms_estimation`'s `datafile` / `initial_year` gate (`E341`); `conditional_forecast`'s `parameter_set` (`E342`), and `conditional_forecast_paths`' mismatched period and value counts and its repeated `var` name (`E343`, `E344`); `markov_switching`'s required options, its chain and regime-count values, its chain order, its `parameters` types, its `restrictions` row shape, its regime bound, its repeated regime pair, its transition probabilities and their sums (`E345`–`E355`); `svar_identification`'s one-block and one-cholesky rules, its repeated lag, its repeated equation, its equation-number bound, its repeated name and its Qi-or-Ri restriction (`E356`–`E362`); `svar`'s choice of one of `coefficients` / `variances` / `constants` with its chain and equation values (`E363`–`E367`); the `ms_*` commands' mutually exclusive regime and filtered-probability options (`E368`–`E371`); and the `prior` statement's `shape`, `mean` / `mode`, `stdev` / `variance` and `domain` rules, the joint head's name count, a head that is not a parameter, and a `corr` head whose two names differ in type (`E372`–`E379`).
- **`E227` reads the `data` statement in file order**: `estimation; data(file='x.csv');` is refused, as Dynare refuses it, while a `data` statement written before the `estimation` silences the refusal on both sides.
- **The parsed family reaches the shared name checks**: an undeclared name in the `svar_identification` body, in a `conditional_forecast_paths` `var` row or in a `prior` head reports `E058`; a `var` row naming an exogenous reports `E317`; a `prior` head naming a parameter reports `E059`; and the trailing name lists of `ms_irf` and `plot_conditional_forecast` report `E239` / `E240` with Dynare's own text.
- **Malformed family shapes** are `E001`, pointed at the token Dynare's parser stops on: a missing or empty option list, an option name the command does not take, a value written in a shape the grammar has no production for, an empty or malformed block body, and the statements `dsample(10, 10);`, `rplot(periods=10);`, `smoother2histval(periods=10);`, `var_remove(alpha);`, `y(1) = 2;` and the dotted heads whose body is not `prior`, `options` or `subsamples`.
- **`E378` also fires on a top-level assignment** (`y = 3;`), where Dynare's `y is not a parameter` is the same sentence its `prior` head prints.

Other changes:
- The `data` statement is parsed: it replaces the presence-only record 0.5.2 kept for the `datafile` gate.
- A line the Dynare lexer reads as native MATLAB text is no longer reported as a parameter assignment with a missing semicolon; `aaaa = 1` followed by `bbbb = 2;` is accepted here, as Dynare accepts it.
- `sbvar`, `svar_identification`, `svar_global_identification_check`, `conditional_forecast_paths`, `plot_conditional_forecast` and the dotted `prior` are in `dynare_list_options`, in option hover and completion, and in command-name completion.

Three shapes stay silent on purpose, because Dynare 7.1 prints no message for them: it aborts on `markov_switching(…, parameters=[<undeclared>])`, on a `restriction` whose expression is not a `coeff(…)` term, and on a `duration` written as a vector.

## v0.5.3

Adds the equation surgery family: the `model_remove(TAGS);` statement and the `model_replace(TAGS); … end;` block now change the model before the checks run, and the refuses they can cause are reported. Codes `E335`–`E337`.

Diagnostics implemented in this release:
- **Equation surgery**: a tag set that matches no equation of the model (`E335`), an excluded equation whose left side is not one endogenous variable and which carries no `endogenous` tag (`E336`, `model_remove` only), and the same endogenous excluded twice by one statement (`E337`). Each refusal points at the equation or the statement in the file you edit.
- **`E256`** (a tag key used twice) also fires on the tag list of a surgery statement, where the preprocessor refuses the list while parsing it.
- **Double-quoted strings**: a tag, a `shock_groups` name, a `bvar_*` string, or an option value written with double quotes is now `E001` — the preprocessor refuses `"…"` with `character unrecognized by lexer` in every one of those positions. Macro directives keep their double quotes, and single quotes are unaffected.
- **Names read as of the statement that names them**: a symbol a later `model_remove` takes out of the model was still endogenous when an earlier statement used it, so `optim_weights` (`E317`) and `ramsey_constraints` (`E321`) written before the removal no longer report a false Error whichever way the symbol left, and `planner_objective` (`E251`) no longer does when the symbol is dropped. A statement written after the removal refuses the name, as before. A constraint's bound is read as of its own row, so a bound a removal re-typed still refuses (`E320`), with the preprocessor's own sentence. The same rule keeps `initval` / `endval` / `histval` / `varobs` entries for a symbol a removal drops from reading as undeclared, `filter_initial_state` reports the timing refusal (`E314`) instead of the undeclared one, and a removed equation's body joins the undeclared-name walk (`E020`).

Other changes:
- The equation list, the counts, and the `[static]` / `[dynamic]` check (`E208`) are the post-removal model, the way 7.1 sees it.
- `model_remove` and `model_replace` are in `dynare_list_options` and in command-name completion.

`exclude_eqs` / `include_eqs` are not covered: they are preprocessor invocation options rather than `.mod` syntax, and the editor never sees them.

## v0.5.2

Covers more of the checks the Dynare preprocessor performs. Codes `E219`–`E334` and `W201`–`W204`. No new command family.

Diagnostics implemented in this release:
- **Commands**: option walks on `estimation` (DSGE-VAR, the `datafile` gate, deprecated options), `sensitivity`, `identification`, `discretionary_policy`, `stoch_simul`, `prior_function` / `posterior_function`, and symbol lists on commands we already record.
- **Blocks we now parse**: `histval`, `estimated_params_init` / `estimated_params_bounds`, `osr_params_bounds`, `epilogue`, `change_type`, `trend_var` / `log_trend_var` / `var(deflator=…)`, `filter_initial_state`, `optim_weights` contents, `ramsey_constraints` body, `external_function`, `init2shocks`, `homotopy_setup`, `shock_groups`.
- **Names, types, macro**: MATLAB/Octave function name as a variable, external function as a bare `var`, macro type mismatches, `log(0)` and division by zero while building an expression, shock type checks (`var` / `stderr` / `cov` / `corr` / `skew`).
- **Duplicates and reuse**: an equation tag twice, an option declared twice, an empty option vector, several `varobs` statements, namespace-qualified misuse, and Warnings for `restriction_fname` and a symbol listed twice.
- **Reserved token**: `dsge_prior_weight` is refused wherever an expression is expected, while a declaration, `estimated_params`, or a model-local `#` may still name it.

Other changes:
- Trend declarations, epilogue helpers, and `external_function(name=…)` names join the duplicate-declaration pass (`E030` / `W031`).
- An unused `varexo_det` no longer reports `E021` (Dynare accepts it), and the `first_deriv_provided` / `second_deriv_provided` function names join the duplicate-declaration pass (`E030` / `W031`).
- `external_function` with the Jacobian and Hessian from the same non-top-level function is refused with Dynare's message (`E334`).

## v0.5.1

Until this release, some diagnostics arrived only by running a local Dynare preprocessor and overlaying its messages (`P###`), which is too late for typing. Some refuses appear only after Dynare rewrites the equations — those points do not land on the `.mod` you edit. 

This release make dygnosis a second preprocessor, which drop the official one and implements all diagnostics itself, except rewrite refuses we cannot map back to the original `.mod`. Dynare 7.1 remains the ground truth.

Diagnostics implemented in this release:
- **Written clash**: Errors Dynare refuses only after transform, when the file you edit is enough to decide: `shocks(surprise)` without `occbin_constraints` (`E178`), `occbin_constraints` with an incompatible command (`E179`), `varexo_det` clashes, two `planner_objective` statements with Ramsey, `shock_paths` mixed with `shocks` / `mshocks` / `endval` / controlled paths.
- **Check-class**: Errors Dynare refuses at check on syntax we already parse: empty model with a run command, perfect-foresight mixed with stochastic context, `model(linear)` nonsmooth ops, policy clashes, solver before setup, `initval`/`endval` order and `all_values_required`.
- Warning Dynare also emits: nonsmooth ops in a stochastic context (`W200`). Isolated `log` in `model(linear)` stays extra Warning (`W140`).

Other changes:
- `dygnosis explain --list` marks each code **shared**, **skipped**, or **added**.
- Include-cycle is extra Warning (`W062`), not Error.

## v0.5.0
- **Occasional constraints (OccBin)**
  - Parse `occbin_constraints` (name / bind / relax / error_*).
  - Keep every source bind/relax equation; count a named pair as one for the equation-count gap.
  - Store all equation tag keys (`bind`, `relax`, `mcp`, `static`, `dynamic`, …). Flag tags are empty strings.
  - Parse complementarity `⟂` / `_|_` on a model equation.
- **Check**
  - Structural OccBin Errors they refuse at check (duplicate blocks, more than two constraints, missing regime / bind / name, …).
  - Warning when an `mcp` tag is used instead of `⟂` (they warn and accept).
- **Catalog / MCP**
  - `dynare_list_options` knows `occbin_constraints`.
  - `dynare_equations` rows include `tags` and, when present, complementarity.

## v0.4.0
- **Expand view**
  - See the model after `@#if` / `@#for` / `@{…}` and includes (effective text).
  - Jump from each counted equation to the source that wrote it (origin).
- **MCP**
  - `dynare_expand`: effective text and origin for each counted equation.
  - `dynare_equations`: additive origin on each counted row.
- **Editor**
  - Command `dynare/showEffectiveModel`: source URI, effective text, and origins.

## v0.3.0
- **Companion files for one `.mod`**
  - Jump from the `.mod` to companions (for example `FILENAME_steadystate.m`, a `datafile=`, a helper `.m`, `run_FILENAME.m`).
  - MCP: `dynare_related_files` (includes and companions).
- **Check changes**
  - `W160` when a named data file or helper path does not resolve.
  - `I050` (no `initval` / `steady_state_model` block) is quiet when `FILENAME_steadystate.m` is present.

## v0.2.0
- **Fewer false Errors**
  - A `method_of_moments(...)` (and other catalog commands) option list was misread as missing semicolons (`E001`). That false Error stopped later checks on the same file, so real MoM / similar work could not start. Those lists are now one statement; only a true missing `;` still fires `E001`.
  - Declaring the same name twice in `var`, `varexo`, or `parameters` is a Warning (`W031`), matching the preprocessor. Declaring one name as two different types, or `#` twice, is still an Error (`E030`).
- **Equations as structured objects**
  - New MCP tool `dynare_equations`: each model equation with index, lhs, rhs, and timing at each use, plus the equation-count gap. For Ramsey / discretionary policy with N instruments, the gap check (`W013`) expects −N equations, not a square file.
  - `dynare_compare_models` diffs equations by index; it no longer lists shared equation text.
  - Editor outline groups endogenous names by timing (predetermined, forward-looking, mixed, static).
- **Check many files**
  - `dygnosis check` can take a directory of `*.mod` files (skips `+` folders). Exit code 1 only when there are Errors, not Warnings.

## v0.1.1

Audit of v0.1.0 against two principles:

- A check is built in instead of delegated to the Dynare preprocessor if (1) it is new; (2) it powers an editor feature; or (3) it helps while typing. The audit finds no check to drop.
- Where checks exist in both dygnosis and the preprocessor, the latter is ground truth. Severity must match (Error or Warning in both), and parity is pinned by honesty tests.

Other changes:

- A check that exists only in dygnosis can only be a Warning or Information. A Warning means the preprocessor would accept the file, but something looks wrong.
- MCP tools are simplified to nine.

## v0.1.0

This release of dygnosis is a Rust rewrite of the thin analysis core from [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) (Python).

### Behaviour vs the Python origin

We aimed to keep the useful diagnostic **codes and messages**, not a line-for-line port of Python internals. Notable differences:

- **Native analysis** — identifiers and spans come from a real expression tree, not regex over masked text. Spans usually underline the bad token, not the whole equation or block.
- **Macros** — inactive `@#if` branches are dropped after expand; `@#for` unrolls fully (Python sometimes kept only the first list value or left dead text in slices).
- **Fewer false alarms** — e.g. call names are not treated as undeclared variables; `1/0` is non-finite (W122), not a “missing value” warning; parameter-range checks look at top-level assignments only, not values inside `steady_state_model`.
- **Where Python was wrong, Rust ships the fix** — e.g. I050 is “no `initval` / `steady_state_model` block” (no fake “compute steady state” solver path); some E001/auto-fix cases report only the missing `;`.
- **One wording across transports** — LSP and MCP no longer disagree on the same warning text where Python did.

### What we left out (numerical / Dynare compute)

These stay with Dynare, not this binary:

- Numerical steady-state solve and “compute steady state” / auto-solve on edit
- Blanchard–Kahn / eigenvalue checks
- Identification and static Jacobian singularity
- Per-equation numerical residuals
- Running a full Dynare/MATLAB session from the language server
- MCP/LSP surfaces that only wrapped those numerics
- Structural block partitioning that needed numeric graph tools (Dulmage–Mendelsohn)
