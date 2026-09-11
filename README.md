# dygnosis

Dygnosis provides language support for Dynare `.mod` files. It parse the `.mod`, report coded diagnostics, and power editor/agent features (hover, rename, format, outline, …). Some quick analysis that is useful while typing is always-on, the Dynare preprocessor is called on save when extra intelligence is needed. Overlap is intentional. The Dynare preprocessor is searched in the following order
1. Editor setting (LSP only) — `preprocessorPath`
2. Environment — `DYNARE_PREPROCESSOR` (full path to the executable)
3. Common installs

## How to use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Run diagnostics on a file, then exit |
| `dygnosis explain <CODE>` | Built-in help for a diagnostic code |
| `dygnosis explain --list` | List all documented codes |
| `dygnosis mcp` | Start the MCP server (stdio) |
| `dygnosis` | Start the language server (stdio) |
| `dygnosis --tcp` | Language server over TCP (debug only; default `127.0.0.1:2087`) |

### Editor (LSP)

Same diagnostics as `check`, plus:

- Hover (declarations, option names)
- Outline and workspace symbols
- Go to definition / type definition
- Find references and rename (including across `@#include` files where the graph is known)
- Completions
- Format document / range
- Code actions and auto-fix where a fix is stored
- Folding, inlay hints, and links into included files
- Optional Dynare preprocessor reconcile on save (see below)

### Agent tools (MCP)

`dygnosis mcp` exposes tools that share the same core:

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Diagnostics for one file |
| `dynare_diagnose_workspace` | Diagnostics with in-memory `@#include` files |
| `dynare_parse_summary` | Structured outline (names and counts) |
| `dynare_explain` | Markdown help for a diagnostic code |
| `dynare_list_diagnostic_codes` | All documented codes and titles |
| `dynare_list_options` | Valid options for a Dynare command (or known commands) |
| `dynare_find_references` / `_workspace` | Whole-word uses of a name (skips comments) |
| `dynare_rename` / `_workspace` | Rename throughout a file or include graph |
| `dynare_auto_fix` | Apply stored diagnostic fixes |
| `dynare_run_preprocessor` | Local Dynare preprocessor in check mode |
| `dynare_model_info` | Names, counts, and equation timing |
| `dynare_compare_models` | Diff two models by names, calibrations, and equations |

## Credits

dygnosis is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.

Suggested citation of the original work:

> Diercks, Anthony, Philip Howard, and Mehrdad Samadi. 2026. "LLMacro: A Language Server for Dynare." Working paper.

The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot) (MIT License).

## License

[GPL-3.0-or-later](LICENSE), matching LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT as upstream.
