# Changelog

Each entry describes the release at the time. Feature names and interfaces can
change in later releases; see Dygnosis Help for current guidance.

## v0.12.0

In VS Code, **Dynare Workspace** shows why a model could not be checked.
Open the reason for check details. **Dygnosis: Show model changes** names both
files or versions on the comparison tab.

**Dygnosis: Select analysis configuration** focuses the current choice.
**Filter Outline by macro activity...** applies to the open model and uses a
default from Settings. In narrow Help, **Topics** opens the topic list while
the article stays visible. Settings uses shorter labels.

**Dygnosis: Set up MCP for this workspace** saves its entry in `.mcp.json`
directly. If that file has unsaved edits, save them before running setup again.

## v0.11.50

In the VS Code editor context menu, Dygnosis actions appear below navigation.
Use **Dygnosis: Choose model owner** or **Dygnosis: Filter analysis by
equation…** from that group. The editor title bar and Command Palette offer
**Select analysis configuration** and **Refresh macro expansion**.
**Rename symbol** keeps its usual position and F2 shortcut. Workspace checks
make known model owners available when an included file opens without its
parents.

Hover explains the `name` and `endogenous` equation-tag keys and shows variable
details on active `endogenous` values. Untitled Dynare editors are checked as
standalone models and show model counts before saving. Help covers model
owners, saved analysis configurations and macro actions.

W011 uses expanded numeric macro values, including in dependent parameter
assignments. On Windows, Rename can update shared files when their parent models
are closed.

## v0.11.49

In VS Code, Call Hierarchy follows the selected model, its includes and macro
expansion. The default macro work limit is now 20 million, so more macro-heavy
models can be analyzed with the default setting.

**Find possible declarations...** and analysis configuration actions are in
the **Dygnosis** editor context submenu. **Expand macros** and **Show model
changes** remain in the editor title bar and Command Palette. The optional
**Expand macros** and **Find references** CodeLens actions and their settings
are removed. Help describes VS Code navigation and editing actions.

## v0.11.48

In the VS Code macro expansion preview, hover shows information about names,
commands and options. Model-local definitions can link to written source.

## v0.11.47

In VS Code, the Source preview opens and refreshes faster for large models
with many macro-generated names. The Explorer view and status bar now use the
name **Dynare Workspace**.

## v0.11.46

Large models with includes open and update faster in the editor and MCP.
**Expand macros** and **Refresh macro expansion** also respond faster for
models with macro loops or repeated includes.

## v0.11.45

Hover and completion explain more Dynare values in their written context,
including equation tags, calibration intervals and nested optimizer and sampler
settings. Hover also explains PAC component kinds and inverse-gamma prior
types, including in inactive macro branches. MCP `dynare_syntax` can look up
these values by command and position.

## v0.11.44

Hover and completion explain fixed option values in their command context,
including in inactive macro branches. MCP `dynare_syntax` can look up the
available meanings and written forms by command and option.

## v0.11.43

MCP equation pages include matches from repeated heterogeneity blocks, and
configured batch results identify their job. Model comparisons page requested
changes and references together.

## v0.11.42

Hover labels long names and omits repeated names in active and inactive code.
Outline and the Find All References picker also label long-name details.

## v0.11.41

Symbol queries, equation inspection and model counts respond faster on large
models in the editor and MCP. **Dygnosis: Show model changes** compares large
parameter calibrations faster, including added, removed and retyped parameters.

## v0.11.40

Hover on a name in an inactive macro branch can show its written declaration
and labels, including from a literal `@#include` path. MCP `dynare_symbol_info`
can inspect the same written declarations and supported model-local definitions.

## v0.11.39

In VS Code, **Dygnosis: Select analysis configuration** accepts simple
hand-written configurations without extra source information. The Command
Palette also offers actions to save or reset a configuration, filter Outline
and workspace checks, and select another model file in **Dygnosis Changes**.
Open Help from the Command Palette or the extension's menu in **Extensions**.

## v0.11.38

Outline shows more named blocks and entries, including names in inactive macro
branches. VS Code dims inactive code for the selected analysis configuration,
including branch previews. Hover describes recognized syntax in those branches.
The dimming can be turned off in Settings.

## v0.11.37

Launch the `dygnosis-mcp` companion directly without arguments. Completed model
analysis is reused by default for each MCP connection. Update existing MCP
launch configurations to use the companion. Set `DYGNOSIS_MCP_REUSE=0` in the
server environment to disable reuse.

## v0.11.36

In VS Code, select the **Dygnosis** file icon theme for colored icons on `.mod`,
`.inc` and `.dyn` files. Macro branches can be previewed from their conditional
directives. **Clear branch previews** restores source conditions while keeping
the other analysis choices.

Status, workspace checks and action messages identify affected files and
causes. MCP results explain affected override targets and omitted actions.

## v0.11.35

Use **Dynare: Select Analysis Configuration** to select equations by tag and
inspect a subset without editing the model. Choose **Include equations** or
**Exclude equations** and enter a tag expression. Compare configurations with
**Dygnosis: Show mod changes**. The selection is a preview, so Rename and
diagnostic fixes are unavailable for it.

**Explain a check** and **Explain this check** are removed. Open Dygnosis Help
and search for the diagnostic code.

## v0.11.34

In the editor, changing the Outline activity filter updates its presentation
without rechecking the model. Editing a file rechecks the models that use it
while results for independent models remain available. Completion, hover,
navigation, model counts and parameter value hints respond faster on large
models.

## v0.11.33

Use an analysis configuration to try macro inputs and branches, explore
inactive source and compare resulting models without changing the file. The
status-bar model counts show **Source** or **Current config** and provide a
link to configuration choices. MCP accepts equivalent request-local choices.

The **Jump to equation...** CodeLens and its setting are removed. The Command
Palette command remains available.

## v0.11.32

In large models, hover, navigation, Outline and parameter value hints respond
faster. Model checks, equation navigation and counts also update faster after
edits.

## v0.11.31

A freeze after rapid edits or settings changes no longer stops model features.
When the macro work limit stops analysis, I211 explains whether the limit can
be increased. The **Configure macro work limit** Quick Fix opens the VS Code
setting when an increase is available.

## v0.11.30

Outline shows named VAR, trend-component, VAR-expectation and PAC model
commands on one row, including in macro expansion previews. **Dygnosis: Show
mod changes** omits Dynare and MATLAB comments from change cards.

## v0.11.29

VS Code's Outline now shows declarations and loop copies in macro expansion
previews. **Go to written source** opens the original model or include file.
Workspace checks can filter for models that were not fully checked.

Project checks now use workspace terminology in commands, settings and Help.
Update `dynare.projectDiagnostics` to `dynare.workspaceDiagnostics` and
`dynare.projectExcludePaths` to `dynare.workspaceExcludePaths`.

## v0.11.28

Outline, breadcrumbs and Go to Symbol show Dynare declarations and structure.
Top-level MATLAB and parameter assignment rows are removed.

MCP removes the `legacy-v2` profile. Callers must use the 12-tool contract and
typed inputs advertised by `tools/list`.

## v0.11.27

Completion, highlighting and syntax help cover more Dynare commands, options,
macro forms and PAC/VAR model syntax. Navigation, references, Rename and
extraction also cover more named structures, including VAR and PAC models.

MCP now advertises 12 focused tools with typed inputs. Existing clients can use
`dygnosis mcp --profile legacy-v2` during this release while migrating to the
new contract. The profile is scheduled for removal in v0.11.28.

## v0.11.26

In the editor and MCP, terminating recursive includes expand with written
origins. Set `dynare.macroWorkLimit` in VS Code or `macro_work_limit` in MCP
model requests to adjust the work available to macro-heavy models. Included
`.mod` files use a unique known model owner automatically.

## v0.11.25

Project checks in VS Code group `.mod` and `.dyn` models by folder and show
saved `.inc` files for navigation. Choose **Ignore this folder** on a folder
row to exclude its model roots from background checks. The toolbar opens
**Configure project exclusions**.

## v0.11.24

The Help **Reference** page lists commands, check codes and command options.
References and Rename include the parameter used by `pac_model`'s `discount`
option.

## v0.11.23

MCP model tools accept supplied text, file maps or saved files for inspection,
formatting and extraction. Rename and other edits return guarded proposals.
Verify their input guards before applying them. Proposals do not write files.

## v0.11.22

Editor and MCP actions explain why an edit or source jump cannot be applied.
Partial MCP results and macro expansion previews identify the cause of missing
results or unavailable navigation.

## v0.11.21

Press F2 or choose **Rename symbol** in the editor context menu to rename a
Dynare symbol across the selected model and its known source files. Rename
withholds an edit when it cannot verify the complete result, the new name
conflicts or source ownership is ambiguous.

## v0.11.20

Choose **Jump to equation...** from the Command Palette or an equation
CodeLens to search counted equations and open their written source. The
**Dynare model** Explorer panel is removed. Model counts remain in the
status-bar hover.

## v0.11.19

In VS Code, fold Dynare blocks, multiline statements, macro loops and individual
conditional branches. Starting lines remain visible when the folded range is
collapsed.

## v0.11.18

Expression value hints remain available after simple MATLAB helper assignments.
Variable hover shows the timing class and the end-of-period convention for
variables declared with `predetermined_variables`.

## v0.11.17

**Dygnosis: Open Help** presents the Dynare workflow from writing and checking
a model to reviewing changes. Feature configuration appears with the relevant
guidance, and the agent section gives an overview of the MCP tools.

## v0.11.16

In **Show mod changes**, a source link on the missing side of an added or
removed item opens the corresponding declaration, command or block when that
context is verified.

## v0.11.15

Formatting preserves native MATLAB command arguments, continuations and
verbatim text in document and selection edits. Model comparison shows changed
expanded command values and keeps both sides of a model block when an option
is added or removed.

## v0.11.14

**Show mod changes** opens a redesigned model comparison with change groups,
Before and After rows, filters and links to written source. **Text diff…**
opens VS Code's file comparison for a captured changed file.

**Expand macros** and **Refresh macro expansion** replace the previous
effective-model command labels.

## v0.11.13

Block backgrounds stay visible while typing and while an updated model result
is pending. The tint updates after a typing pause and clears when disabled.

## v0.11.12

Model-local variables have completion, expression hover, definition navigation,
references and safe Rename in the editor. Syntax checks cover their definitions
and uses. MCP model inspection and extraction include model-local facts.

## v0.11.11

After a line-start `@#`, **Trigger Suggest** lists macro directives and inserts
the selected name. Typing `#` after `@` opens the same list.

## v0.11.10

**Open changes** compares the active model with another `.mod` file or a Git
revision, branch or tag. Historical comparisons use includes from the same
revision. MCP `dynare_compare_models` also accepts Git and saved working inputs.

## v0.11.9

Macro expansion handles more Dynare expressions, includes and repeated execution.
**Show effective model** preserves source spacing and marks expanded text.
**Go to written source** works on written text in the preview, including
macro-generated names. MATLAB text after a native entry stays on that line,
and Quick Fix applies to the selected check.

I210 and W070 are removed. The bundled agent skill is renamed `use-dynare` and
directs numerical runs to the MATLAB Agentic Toolkit.

## v0.11.8

In the editor, **Add equation tags** and **Add long names** fill missing model
metadata, including in included files. Completion offers equation tags and
symbol long names. Macro interpolation expands inside quoted equation tags
and long names. Variable timing and model summaries follow the end-of-period
convention for valid `predetermined_variables` declarations.

MCP withholds equation counts when required inputs are incomplete and waits
for complete inputs before comparing models.

## v0.11.7

**Dygnosis: Open Help** opens an offline reference in VS Code with search,
examples and feature guides. Dygnosis Help replaces the previous user guides.

The `check` and `explain` CLI commands are removed. Use editor diagnostics or
`dynare_workspace_diagnose` to check models, and Help or `dynare_explain` for
check explanations.

## v0.11.6

The Windows x64 VSIX is about 5% smaller and the standalone archive about 6%
smaller than in v0.11.5. W011 parameter assignment warnings follow written
source order.

## v0.11.5

Model-expression diagnostics cover heterogeneous and replacement equations,
policy expressions, OccBin constraints and PAC/VAR expressions. External
function calls in those expressions are checked for a prior declaration and
the declared argument count.

## v0.11.4

Model checks cover heterogeneous shock duplicates, steady-state assignments
and qualified external function calls. Includes in inactive macro branches and
empty loops no longer produce missing-file or syntax errors.

## v0.11.3

VS Code checks saved `.mod` files in the project even when they are closed.
Progress appears in the status bar. Use **Dygnosis: Recheck project** to run
discovery again and `dynare.projectExcludePaths` to exclude roots from
background checks. `dynare.projectDiagnostics` is enabled by default.

## v0.11.2

**Diff with…** compares the active model with another model, with filters and
links to written source. The effective-model preview can jump to written
equations and the macro directives that produced them.

**Dygnosis: Set up MCP for this project** writes a project-local `.mcp.json`
and installs the bundled engine at a stable path for agents.

## v0.11.1

The first Dygnosis VS Code extension checks open `.mod`, `.dyn` and `.inc`
files and their includes. It adds model counts, equation navigation, a Dynare
model view and a read-only **Show effective model** preview. The extension
requires VS Code 1.102 or newer and includes the engine for Windows, macOS and
Linux on x64 and ARM64.

## v0.11.0

Editor and agent tools show more model information and written source locations.
Hover and completion show long names and TeX names, and expression value hints
show known values for eligible assignments. Duplicate and include-cycle
diagnostics link to the earlier written location.

## v0.10.2

Model checks, counts and hover follow symbol types after `change_type`.
Subsample definitions and uses follow macro execution order. Extraction keeps
the type changes needed by selected names.

## v0.10.1

Macro functions, filtered loops and tuple loops expand in diagnostics and model
views with written origins. Diagnostics and references in included files point
to the included source. When expansion is incomplete, model facts that depend
on it are withheld.

## v0.10.0

Agents can format models with `dynare_format`, extract named equation groups
with `dynare_extract`, and check multiple models with
`dynare_workspace_diagnose`. Model comparison includes named equations,
heterogeneity dimensions and symbol metadata.

Comment-based diagnostic silencing is removed. Parameter-value inlay hints
and the model-count CodeLens are also removed.

## v0.9.0

Dynare 7.2 heterogeneity dimensions, declarations, equations and shocks appear
in model checks and inspection. `dynare_model_info` reports aggregate and
per-dimension counts, and model comparison shows heterogeneous shock changes.

## v0.8.0

VAR, trend-component and PAC models have syntax help and checks for their
written commands, equations and options. Completion, hover and
`dynare_list_options` describe the supported Dynare 7.2 forms.

## v0.7.0

Model checks cover more written shocks, paths and subsample settings. Model
comparison shows changes to shock and simulation setup, including written
values, timing and source locations.

## v0.6.1

Language checks use Dynare 7.2. The `histval_file` options `nobs` and
`last_simulation_period` are no longer offered and are reported as syntax
errors. `initval_file` continues to accept them.

## v0.6.0

Model checks cover method-of-moments settings, matched moments and IRFs, and
IRF and moment calibration blocks. Completion and `dynare_list_options` also
include their supported options.

## v0.5.5

Model checks report selected problems that Dynare finds when writing MATLAB
output, including unsupported `load_params_and_steady_state` names, external
functions inside `steady_state` and repeated `shock_groups` labels.

## v0.5.4

Model checks cover MS-SBVAR, Markov switching, conditional forecasts and prior
statements. Completion and option help include commands such as `sbvar` and
`plot_conditional_forecast`.

## v0.5.3

`model_remove` and `model_replace` change the inspected model before checks
run. Equation lists and counts reflect the resulting model, and invalid tag
selections are reported.

## v0.5.2

Model checks cover more Dynare commands, declarations, blocks and expressions,
including `histval`, `external_function` and `ramsey_constraints`.

## v0.5.1

Dygnosis checks edited models directly without launching the Dynare
preprocessor. Supported Dynare refusals and warnings include incompatible
OccBin, shock and simulation setup.

## v0.5.0

OccBin model checks cover `occbin_constraints`, equation tags and
complementarity conditions. `dynare_equations` includes the tags and any
complementarity on each equation.

## v0.4.0

The editor's effective-model view shows macro expansion and includes, with
links from generated equations to written source. MCP `dynare_expand` returns
the expanded text and equation origins.

## v0.3.0

From a `.mod` file, navigate to related files such as a steady-state function,
a data file or a MATLAB helper. MCP `dynare_related_files` lists includes and
companion files. Missing named data files and helpers can produce W160.

## v0.2.0

MCP `dynare_equations` returns structured model equations and timing, and the
editor Outline groups endogenous names by timing. `dygnosis check` can check a
directory of `.mod` files. Option lists such as `method_of_moments(...)` no
longer report false missing semicolon errors.

## v0.1.1

Shared diagnostic severities follow Dynare's errors and warnings. Additional
Dygnosis guidance is identified separately. The MCP interface is simplified
to nine tools.

## v0.1.0

Dygnosis provides shared editor and agent diagnostics for Dynare models. Its
Rust analysis core is based on
[LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP).
Numerical model solving remains with Dynare and MATLAB or Octave.
