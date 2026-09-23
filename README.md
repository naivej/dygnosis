*This project is under active development.*

![dygnosis](media/logo_s.png) is a second Dynare preprocessor living in your editor that

- powers MCP for agents and LSP for humans, with code intelligence while typing;
- internally rewrites the model and maps checks on the transformed model back to the original `.mod`;
- checks almost all issues covered by the Dynare (7.2) preprocessor, and more.

Limitations by design

- **Checks on a rewritten model are not reported.** Dynare rearranges your equations before some of its checks, adding extra variables and turning leads and lags around. When a check complains about that version, there is no matching place in the file you edit, so dygnosis stays quiet.
- **A few Dynare messages have no match here.** One is about the MATLAB code Dynare writes out, not about your model. In others Dynare crashes instead of printing anything. Either way there is nothing to show you.
- **Checks that depend on how you run Dynare are skipped.** Some come from command-line options, others need a compiler on your machine. They say nothing about the `.mod` file.
- **Numbers are left to MATLAB.** Steady state, stability, and whether the model solves are computed there, not here. Use the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit) for seamless integration with MATLAB.

Shock checks read written `shocks`, `mshocks`, `heteroskedastic_shocks`, and `shock_paths` instructions, including periods and learning dates. For a selected `stoch_simul(irf_shocks=(…))` list, W060 names each shock missing a written stochastic size. With an explicit positive `irf` and no selected list, it warns only when no plain exogenous shock has a written size at all. It stays quiet when the relevant plain exogenous shock has an estimated standard error, external code may supply its size, or macros and includes leave the setup uncertain. An observed-variable measurement-error estimate alone does not silence it. `irf=0`, a bare `varexo` declaration, and a bare `stoch_simul` stay quiet. These checks do not calculate IRFs.

Subsample checks cover named ranges, copies, and related `prior` / `options` statements. They report a missing declaration or range name and a target type Dynare would reject when writing its output. Diagnostics also catch a variable used after `var_remove`, a denominator such as `y/(y-y)`, and invalid command or DATE syntax.

Compare reports written shock setup changes in `shock_setup_changes` and a **Shock setup changes** markdown section: values, periods, learning dates, controls, and overwritten instructions. It gives source locations when the parsed text matches the file you supplied; an included instruction has no misleading root-file location. It does not calculate a realized shock path.

Changed initial and terminal values, dates, database references, and heteroskedastic data settings appear beside the relevant shock setup. Changes to `shock_groups` and `init2shocks` appear in a separate **Shock analysis setup** section.

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
