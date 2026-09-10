# dygnosis

Dynare language tooling for `.mod` files. It is under active development with many new features to come.

## Quick start

```bash
cargo build
dygnosis check path/to/model.mod  # run diagnostics on a .mod file, then exit
dygnosis explain E010             # built-in help for a diagnostic code
dygnosis explain --list           # list all documented codes
dygnosis mcp                      # stdio MCP (diagnose, refs, rename, preprocessor, model info, compare)
dygnosis                          # stdio LSP (hover, outline, rename, format)
dygnosis --tcp                    # TCP LSP (debug only; default 127.0.0.1:2087)
```

Requires a Rust toolchain. Optional: a Dynare install for preprocessor-backed `check`, save, and MCP diagnose. Set `DYNARE_PREPROCESSOR` to the `dynare-preprocessor` executable, or use a common install such as `C:\dynare\7.1`.

## Credits

dygnosis is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard, and Mehrdad Samadi. Diagnostic codes, check and explain surfaces, and the thin analysis design come from that work. The original repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.

Suggested citation of the original work:

> Diercks, Anthony, Philip Howard, and Mehrdad Samadi. 2026. "LLMacro: A Language Server for Dynare." Working paper.

The agent skill under `.agents/skills/dynare-copilot/` is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot) (MIT License).

## License

[GPL-3.0-or-later](LICENSE), matching LLMacro-Dynare-LSP. The vendored dynare-copilot skill remains MIT as upstream.
