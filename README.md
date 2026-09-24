*This project is under active development.*

![dygnosis](media/logo_s.png) is a second Dynare preprocessor living in your editor that

- powers MCP for agents and LSP for humans, with code intelligence while typing;
- checks written `.mod` files against many Dynare 7.2 preprocessor refusals before MATLAB;
- includes extra warnings for issues Dynare does not report.

Limitations by design

- **Checks on a rewritten model are not reported.** Dynare rearranges your equations before some of its checks, adding extra variables and turning leads and lags around. When a check complains about that version, there is no matching place in the file you edit, so dygnosis stays quiet.
- **A few Dynare messages have no match here.** One is about the MATLAB code Dynare writes out, not about your model. In others Dynare crashes instead of printing anything. Either way there is nothing to show you.
- **Checks that depend on how you run Dynare are skipped.** Some come from command-line options, others need a compiler on your machine. They say nothing about the `.mod` file.
- **Numbers are left to MATLAB.** Steady state, stability, and whether the model solves are computed there, not here. Use the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit) for seamless integration with MATLAB.

Semi-structural support covers `var_model`, `trend_component_model`, `var_expectation_model`, `pac_model`, `pac_target_info`, and their named expectation operators. `deterministic_trends` is an independent block. LSP and MCP share the diagnostic core. LSP hover and completion, and MCP `dynare_list_options`, use the same command catalog.

## How to use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Full diagnostics, then exit |
| `dygnosis check <dir>` | Recurse `*.mod` (skip `+` folders); one summary line; exit 1 on errors, not warnings |
| `dygnosis explain <CODE>` | Help for a diagnostic code |
| `dygnosis explain --list` | List all diagnostic codes, classified as shared, skipped, or added relative to Dynare |
| `dygnosis mcp` | Start the MCP server (stdio) |
| `dygnosis` | Start the language server (stdio) |
| `dygnosis --tcp` | Language server over TCP (debug only; default `127.0.0.1:2087`) |

### Editor (LSP)

- Hover (declarations, option names)
- Outline (endogenous grouped by timing class) and workspace symbols
- Go to definition / type definition
- Find references and rename (including across `@#include` files where the graph is known)
- Completions
- Format document / range
- Code actions and auto-fix where a fix is stored
- Folding, inlay hints, and links into `@#include` files and companions (e.g. `FILENAME_steadystate.m`)
- Show the effective model (`dynare/showEffectiveModel`): text after `@#if` / `@#for` / `@{…}` and includes, with origin jump from each counted equation
- Diagnostics while typing and on save

### MCP tools

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Full diagnostics |
| `dynare_model_info` | Names, counts, timing, block flags |
| `dynare_compare_models` | Diff names, parameter values, equations, and written shock setup |
| `dynare_find_references` | Uses of a name |
| `dynare_rename` | Rename a name |
| `dynare_auto_fix` | Stored fixes, one file |
| `dynare_explain` | Help for a diagnostic code |
| `dynare_list_diagnostic_codes` | List all diagnostic codes, classified as shared, skipped, or added relative to Dynare |
| `dynare_list_options` | Options for a command |
| `dynare_equations` | Equations with lhs, rhs, timing, tags, complementarity, origin, and the equation-count gap |
| `dynare_related_files` | Includes and companions for the active `.mod` |
| `dynare_expand` | Effective text after `@#if` / `@#for` / `@{…}` and includes, plus origin for each counted equation |

## Credits

1. dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation-object, tag, extract, and related editor ideas are informed by [modBuilder](https://git.dynare.org/Dynare/modBuilder) (Dynare Team), a MATLAB API for building `.mod` files.
3. The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

[GPL-3.0-or-later](LICENSE), matching Dynare and LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT, matching upstream.
