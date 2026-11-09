# Changelog

Each entry describes the release at the time. Feature names and interfaces can
change in later releases; see Dygnosis Help for current guidance.

## v0.11.50

The editor context menu is redesigned to put Dygnosis actions together without
a submenu. It adds actions to choose a model owner and filter analysis by
equation. Configuration selection and macro preview refresh are removed from
the menu; they remain in the editor title bar and Command Palette. Rename stays
in its usual position and on F2. Workspace checks also make known model owners
available when an included file is opened without its parent models.

## v0.11.49

Editor navigation and Help are streamlined, and Call Hierarchy follows the
selected model and its macro expansion. The default macro work limit increases
to 20 million for larger models. The optional Expand macros and Find
references CodeLens actions are removed; their actions remain available
elsewhere.

## v0.11.48

Macro expansion previews now support hover, so names, commands and options can
be inspected without returning to the written source.

## v0.11.47

Source preview opens and refreshes faster for large models with many
macro-generated names. Workspace checks also use the name Dynare Workspace.

## v0.11.46

Large models with includes open, update and expand faster in the editor and
MCP by reusing completed macro work.

## v0.11.45

Contextual value help covers more written Dynare syntax, including equation
tags and nested settings, in both the editor and MCP.

## v0.11.44

Fixed option values have contextual explanations in hover, completion and MCP,
including in inactive macro branches.

## v0.11.43

MCP results retain more complete model information: equation pages include
matches from repeated heterogeneity blocks, configured batch results identify
their job, and comparisons page changes and references together.

## v0.11.42

Name information is easier to read across hover, Outline and the references
picker, with less repetition and clearer long names.

## v0.11.41

Symbol and equation inspection, model counts and comparisons respond faster on
large models in the editor and MCP.

## v0.11.40

Written declarations and model-local definitions can be inspected in inactive
macro branches through hover and MCP, including supported literal includes.

## v0.11.39

Analysis configurations and commands are easier to find and use. Simple
hand-written configurations work without extra source information, and more
actions and Help are available from the Command Palette.

## v0.11.38

Inactive macro branches are easier to explore: Outline and hover show more
written content, and inactive code is dimmed for the selected configuration.

## v0.11.37

MCP reuses completed model analysis by default, making repeated queries more
efficient. Launch the `dygnosis-mcp` companion directly without arguments;
update existing MCP launch configurations. Set `DYGNOSIS_MCP_REUSE=0` to
disable reuse.

## v0.11.36

Editor and MCP actions give clearer reasons when results are incomplete or an
action cannot proceed. Macro branches can also be previewed directly from
their directives.

## v0.11.35

Equation selection lets users analyze a tagged subset without editing the
model, including when comparing configurations. This is an analysis preview,
so Rename and diagnostic fixes are unavailable for the selection.

## v0.11.34

Editor features respond sooner on large models by reusing analysis. Affected
models are rechecked after edits while independent results remain available.

## v0.11.33

Analysis configurations let users try macro inputs and branches, explore
inactive source and compare the resulting models without changing the file.
MCP accepts equivalent choices. The Jump to equation CodeLens is removed; the
command remains available.

## v0.11.32

Large-model editing is more responsive, from checking after edits to
navigation, hover, Outline and parameter hints.

## v0.11.31

Large-model analysis is more reliable and responsive: a freeze after rapid
changes is fixed, and macro work-limit messages explain how to continue.

## v0.11.30

Named VAR and PAC models are easier to browse in Outline. Model Diff also
focuses on model changes by ignoring comments.

## v0.11.29

Workspace checks adopt VS Code's workspace terminology and make incomplete
checks easier to find; macro expansion previews gain Outline support. Update
saved settings to `dynare.workspaceDiagnostics` and
`dynare.workspaceExcludePaths`.

## v0.11.28

Outline and symbol navigation focus on Dynare declarations and structure
rather than top-level MATLAB and parameter assignments. MCP removes
`legacy-v2`; callers must use the 12-tool contract and typed inputs advertised
by `tools/list`.

## v0.11.27

Dynare syntax support is broader across editor features and extraction. MCP is
reorganized around 12 focused tools; a temporary `legacy-v2` profile remains
available for migration.

## v0.11.26

Macro-heavy models have better support for recursive includes and configurable
work limits in the editor and MCP. Included model files also use a unique
known owner automatically.

## v0.11.25

Workspace checks organize models by folder, show saved include files for
navigation and let users exclude folders from background checks.

## v0.11.24

Help reference navigation is improved, and References and Rename recognize the
parameter used by `pac_model`'s `discount` option.

## v0.11.23

MCP model inspection and edits use consistent source inputs and guarded
proposals. Agents can inspect supplied text, file maps or saved files;
proposed edits do not write files.

## v0.11.22

Editor and MCP actions explain why a request is refused or a result is
partial, including when an edit or source jump is no longer available.

## v0.11.21

Rename works across the selected model and its known source files. Use F2 or
the editor context menu; incomplete or ambiguous edits are withheld.

## v0.11.20

Equation navigation moves to one searchable picker, and the Dynare model
Explorer panel is removed. Model counts remain available in the status-bar
hover.

## v0.11.19

Editor folding covers Dynare blocks, statements and macro branches, making
long model files easier to navigate.

## v0.11.18

Parameter value hints can follow simple MATLAB helper assignments. Variable
hover also shows timing and predetermined conventions.

## v0.11.17

Help is organized around the Dynare workflow, bringing feature guidance,
configuration and agent tools together.

## v0.11.16

Model comparison can open source context for the missing side of an added or
removed item when that context is verified.

## v0.11.15

Formatting and model comparison handle native MATLAB text, macro values and
block changes more reliably.

## v0.11.14

Model Diff is redesigned around semantic model changes and captured source,
with source navigation and access to VS Code's text diff. Macro expansion
commands also use clearer names.

## v0.11.13

Block backgrounds stay visible while typing and while updated analysis is
pending.

## v0.11.12

Model-local variables gain consistent syntax checks and editor support for
completion, navigation, references and safe Rename. MCP inspection and
extraction use the same model-local facts.

## v0.11.11

Macro directives can be completed after `@#`, making them easier to discover
and enter.

## v0.11.10

Model comparison extends to Git history and other files in the editor and MCP.
Historical comparisons use the included files from the selected revision.

## v0.11.9

Macro expansion, native MATLAB handling and editor fixes are more reliable
across includes and repeated macro execution. I210 and W070 are removed
because their triggers do not establish a problem; the bundled agent skill is
renamed `use-dynare`.

## v0.11.8

Editor actions and model checks better handle metadata, quoted macro values
and predetermined-variable timing. MCP withholds authoritative counts and
comparisons when required inputs are incomplete.

## v0.11.7

Offline Dygnosis Help replaces the previous user guides and brings feature and
diagnostic references into VS Code. The `check` and `explain` CLI commands are
removed; use editor or MCP diagnostics for checks, and Help or MCP for
explanations.

## v0.11.6

The Windows x64 package is smaller following a maintenance and binary-size
review.

## v0.11.5

Model-expression diagnostics cover more Dynare constructs, including
heterogeneous equations, policy expressions and PAC/VAR models.

## v0.11.4

Diagnostics better handle inactive includes, steady-state assignments,
heterogeneous shocks and qualified external functions.

## v0.11.3

Workspace model checks are enabled by default for saved files, with progress,
recheck and exclusion controls.

## v0.11.2

Structural model comparison and macro preview navigation connect model facts
and generated equations to written source. Project-local MCP setup provides a
stable executable path for agents.

## v0.11.1

The first VS Code extension packages Dynare editing, model counts,
diagnostics, macro previews and agent tools for Windows, macOS and Linux.

## v0.11.0

The shared language server and agent tools gain richer model information,
source locations, semantic colors and expression-value hints.

## v0.10.2

A stability pass makes name types, source order, macro execution and
extraction more consistent across diagnostics and model views.

## v0.10.1

Language checks and macro expansion cover more of Dynare 7.2, with
included-file locations retained and unverifiable model facts withheld.

## v0.10.0

Editor and agent workflows expand with formatting, extraction, multi-model
checks and richer comparison. Comment-based diagnostic silencing is removed so
editor, CLI and agent results use the same checks.

## v0.9.0

Dynare 7.2 heterogeneity support adds syntax, checks, model information and
shock comparison while keeping aggregate and per-dimension facts separate.

## v0.8.0

VAR, trend-component and PAC model syntax gains help and applicable checks for
semi-structural models.

## v0.7.0

Shock, path and subsample support expands, and model comparison shows changes
to written shock and simulation setup.

## v0.6.1

Language checks and verification move to Dynare 7.2, with affected option and
duplicate checks aligned to the new pin.

## v0.6.0

Method-of-moments, matched moment and IRF, and calibration syntax gain
applicable checks.

## v0.5.5

Selected problems that Dynare reports when writing MATLAB output can be
checked from the edited source.

## v0.5.4

MS-SBVAR, Markov switching, conditional forecast and prior syntax gain
applicable checks.

## v0.5.3

Equation removal and replacement are reflected in the inspected and checked
model through `model_remove` and `model_replace`.

## v0.5.2

Diagnostic coverage expands across existing Dynare commands, declarations,
blocks and expressions.

## v0.5.1

Diagnostics run in Dygnosis without starting the official preprocessor, while
Dynare remains the reference for supported refusals and warnings.

## v0.5.0

OccBin support adds syntax and structural checks for occasionally binding
constraints, including equation tags and complementarity.

## v0.4.0

Macro expansion can be inspected in the editor and MCP, with navigation from
generated equations to their written origins.

## v0.3.0

Companion-file discovery and navigation support models that use several files.

## v0.2.0

A more accurate model representation exposes structured equations for editor
and agent workflows and reduces false diagnostics.

## v0.1.1

Shared diagnostic severities are aligned with Dynare, and additional Dygnosis
guidance is distinguished from preprocessor refusals.

## v0.1.0

The Rust analysis core is introduced, based on
[LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP), with
shared editor and agent diagnostics. Numerical model solving remains with
Dynare and MATLAB or Octave.
