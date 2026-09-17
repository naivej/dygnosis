![dygnosis](media/logo_s.png) provides language support for Dynare. It parses the `.mod`, reports model diagnostics, and powers editor/agent features (hover, rename, format, outline, …).

*This project is under active development.*

## Why not the Dynare preprocessor

Dynare’s preprocessor reads a `.mod` and writes MATLAB or Octave code. Dygnosis is a second preprocessor for the same language. It stops before MATLAB. It does not run Dynare’s preprocessor.

**Difference.** The model in the editor is the `.mod` you write. Before MATLAB, Dynare’s preprocessor rewrites equations: it adds helper variables, inlines `#` locals, and substitutes constants. That rewritten form is not a file you edit, and Dygnosis does not show it. Dygnosis adds checks Dynare does not do, and it reports problems while you type.

**Parity.** Dynare’s preprocessor is still the ground truth for whether a file is refused or warned before MATLAB. Dygnosis does not emit an Error on a file Dynare would accept at that point.

**Limitation.** Some Dynare refusals exist only after that rewrite. If Dygnosis cannot point at the original `.mod`, it stays silent.

`dygnosis explain --list` lists every documented code as **emit** (we report it), **skip** (documented, not emitted yet), or **added** (ours; Dynare never reports it). `dygnosis explain <CODE>` prints help for one code.

## How to use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Full diagnostics, then exit |
| `dygnosis check <dir>` | Recurse `*.mod` (skip `+` folders); one summary line; exit 1 on errors, not warnings |
| `dygnosis explain <CODE>` | Built-in help for a diagnostic code |
| `dygnosis explain --list` | List documented codes (emit / skip / added) |
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
- Show the expanded model (`dynare/showEffectiveModel`): effective text after `@#if` / `@#for` / `@{…}` and includes, with origin jump from each counted equation
- Diagnostics while typing and on save

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
| `dynare_list_diagnostic_codes` | Documented codes (emit / skip / added) |
| `dynare_list_options` | Options for a command (including the `occbin_constraints` block) |
| `dynare_equations` | Equations with lhs, rhs, timing, tags, complementarity, origin, and the equation-count gap |
| `dynare_related_files` | Includes and companions for the active `.mod` |
| `dynare_expand` | Effective text after `@#if` / `@#for` / `@{…}` and includes, plus origin for each counted equation |

## Credits

1. dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation-object, tag, extract, and related editor ideas are informed by [modBuilder](https://git.dynare.org/Dynare/modBuilder) (Dynare Team), a MATLAB API for building `.mod` files. Language Errors still follow the Dynare preprocessor.
3. The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).


## License

[GPL-3.0-or-later](LICENSE), matching Dynare and LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT as upstream.
