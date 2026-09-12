# dygnosis

Dygnosis provides language support for Dynare `.mod` files. It parses the `.mod`, reports coded diagnostics, and powers editor/agent features (hover, rename, format, outline, …).

Own checks run while typing (LSP `did_change`). A full check — own diagnostics, the Dynare preprocessor, and the safety gate — runs on `dygnosis check`, LSP save, and MCP `dynare_diagnose`.

**Error:** they would refuse the file. **Warning:** they would accept, but something looks wrong (theirs, or an extra check they do not do). **Information:** a note, not a smell.

After a preprocessor run, they win (not line by line). On refuse, same-ground Errors yield to its wording. On accept, no Error of ours remains. When they warn, same-ground Warnings yield to theirs; extra Warnings stay. If the preprocessor binary is missing, only own checks run.

The Dynare preprocessor is searched in the following order:

1. Editor setting (LSP only) — `preprocessorPath`
2. Environment — `DYNARE_PREPROCESSOR` (full path to the executable)
3. Common installs

## How to use

### Command line

| Command | Purpose |
|---------|---------|
| `dygnosis check <file.mod>` | Full check (own diagnostics + preprocessor + safety gate), then exit |
| `dygnosis explain <CODE>` | Built-in help for a diagnostic code |
| `dygnosis explain --list` | List all documented codes |
| `dygnosis mcp` | Start the MCP server (stdio) |
| `dygnosis` | Start the language server (stdio) |
| `dygnosis --tcp` | Language server over TCP (debug only; default `127.0.0.1:2087`) |

### Editor (LSP)

Own checks while typing, plus:

- Hover (declarations, option names)
- Outline and workspace symbols
- Go to definition / type definition
- Find references and rename (including across `@#include` files where the graph is known)
- Completions
- Format document / range
- Code actions and auto-fix where a fix is stored
- Folding, inlay hints, and links into included files
- Full check on save (own + preprocessor + safety gate)

### Agent tools (MCP)

`dygnosis mcp` exposes tools that share the same core. `dynare_diagnose` is the only full check. The other eight do not run the preprocessor.

| Tool | Purpose |
|------|---------|
| `dynare_diagnose` | Own diagnostics + preprocessor + safety gate (the only full check) |
| `dynare_model_info` | Names, counts, timing, block flags |
| `dynare_compare_models` | Diff two models |
| `dynare_find_references` | Uses of a name |
| `dynare_rename` | Rename a name |
| `dynare_auto_fix` | Stored fixes, one file |
| `dynare_explain` | Help for a diagnostic code |
| `dynare_list_diagnostic_codes` | Documented codes |
| `dynare_list_options` | Options for a command |

## Credits

dygnosis is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.

Suggested citation of the original work:

> Diercks, Anthony, Philip Howard, and Mehrdad Samadi. 2026. "LLMacro: A Language Server for Dynare." Working paper.

The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot) (MIT License).

## License

[GPL-3.0-or-later](LICENSE), matching LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT as upstream.
