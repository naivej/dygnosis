![dygnosis](media/logo_s.png) provides language support for Dynare. It parses the `.mod`, reports model diagnostics, and powers editor/agent features (hover, rename, format, outline, …).

- It adds new checks beyond the Dynare preprocessor.
- Overlap with the preprocessor is intentional when those checks power an editor feature or help while typing.
- It does not build in every preprocessor check; it invokes the preprocessor instead.

The Dynare preprocessor is searched in the following order:

1. Editor setting (LSP only) — `preprocessorPath`
2. Environment — `DYNARE_PREPROCESSOR` (full path to the executable)
3. Common installs

*This project is under active development.*

## How to use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Full diagnostics, then exit |
| `dygnosis check <dir>` | Recurse `*.mod` (skip `+` folders); one summary line; exit 1 on errors, not warnings |
| `dygnosis explain <CODE>` | Built-in help for a diagnostic code |
| `dygnosis explain --list` | List all documented codes |
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
- Folding, inlay hints, and links into included files
- Quick intelligence while typing, full diagnostics on save

### Agent tools (MCP)

`dygnosis mcp` exposes tools that share the same core.

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Full diagnostics |
| `dynare_model_info` | Names, counts, timing, block flags |
| `dynare_compare_models` | Diff two models by names and equation index |
| `dynare_find_references` | Uses of a name |
| `dynare_rename` | Rename a name |
| `dynare_auto_fix` | Stored fixes, one file |
| `dynare_explain` | Help for a diagnostic code |
| `dynare_list_diagnostic_codes` | Documented codes |
| `dynare_list_options` | Options for a command |
| `dynare_equations` | Equations with lhs, rhs, timing, and the equation-count gap |

## Credits

1. dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation-object, tag, extract, and related editor ideas are informed by [modBuilder](https://git.dynare.org/Dynare/modBuilder) (Dynare Team), a MATLAB API for building `.mod` files. Language Errors still follow the Dynare preprocessor.
3. The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).


## License

[GPL-3.0-or-later](LICENSE), matching Dynare and LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT as upstream.
