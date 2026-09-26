*This project is under active development.*

![dygnosis](media/logo_s.png) is a second Dynare preprocessor living in your editor that

- powers MCP for agents and LSP for humans, with code intelligence while typing;
- catches many problems Dynare 7.2 would report before MATLAB runs and points to them in your `.mod` file;
- adds warnings for possible problems Dynare does not report.

Limitations by design

- **Problems after equation rewriting.** Dynare changes equations and adds helper variables before some checks. Dygnosis can report a problem found at that stage when the `.mod` you wrote is enough to decide it. It stays quiet when the problem depends on equations Dynare creates and cannot be tied back to your file.
- **Some Dynare messages cannot be shown.** A few concern MATLAB or Octave code Dynare writes rather than your `.mod` file; other cases crash without an error message. Dygnosis does not invent a diagnostic for those cases.
- **Run-dependent checks.** Dygnosis does not infer checks that depend on Dynare command-line options or a compiler installed on your machine.
- **Numerical results.** MATLAB or Octave computes steady states, stability, and whether the model solves. Dygnosis stops before those calculations. For editor and agent integration of MATLAB, see the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

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
- Folding and links into `@#include` files and companions (e.g. `FILENAME_steadystate.m`)
- Show the effective model (`dynare/showEffectiveModel`): text after `@#if` / `@#for` / `@{…}` and includes, with origin jump from each counted equation
- Diagnostics while typing and on save.

### MCP tools

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Full diagnostics |
| `dynare_model_info` | Names, counts, timing, block flags |
| `dynare_compare_models` | Diff names, symbol kind and metadata, parameter values, equations (by name and tags), and written shock setup |
| `dynare_find_references` | Uses of a name |
| `dynare_rename` | Rename a name |
| `dynare_auto_fix` | Stored fixes, one file |
| `dynare_explain` | Help for a diagnostic code |
| `dynare_list_diagnostic_codes` | List all diagnostic codes, classified as shared, skipped, or added relative to Dynare |
| `dynare_list_options` | Options for a command |
| `dynare_equations` | Equations with text, timing, tags, complementarity, origin, and the equation-count gap |
| `dynare_related_files` | Includes and companions for the active `.mod` |
| `dynare_expand` | Effective text after `@#if` / `@#for` / `@{…}` and includes, plus origin for each counted equation |
| `dynare_format` | Format a `.mod` file with the editor's rules. Returns the full text only when it changes |

## Credits

1. dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation-object, tag, extract, and related editor ideas are informed by [modBuilder](https://git.dynare.org/Dynare/modBuilder) (Dynare Team), a MATLAB API for building `.mod` files.
3. The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

[GPL-3.0-or-later](LICENSE), matching Dynare and LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT, matching upstream.
