![Dygnosis](media/logo_s.png) 

Dygnosis provides language support for Dynare, acting as a second preprocessor that is deeply integrated into the editor and agent workflow via LSP and MCP.

- Edit with live diagnostics, hover, completion, document or selection formatting, fixes, and commented shock templates.
- Navigate with references and rename across known includes, plus outline, workspace symbols, folding, syntax colors, and links to related files.
- Inspect model counts, equations, includes, and expanded source, with jumps back to the written equations.
- Compare a model with Git history or another `.mod` file; check unopened saved models in a project.

Dygnosis works out of the box when installed from the VS Code Marketplace. Run **Dygnosis: Open Help** to learn more.

For MCP without VS Code, download the native Rust binary from [GitHub Releases](https://github.com/naivej/dygnosis/releases) and follow [MCP setup](help/get-started.md#mcp-without-vs-code).

## Limits

- Language support follows **Dynare 7.2**. Some pre-MATLAB checks are outside coverage; use Dynare for final validation.
- Unsupported macro expressions are flagged as incomplete. Affected checks are withheld and model views are marked incomplete.
- Dygnosis does not launch Dynare, MATLAB or Octave. Numerical results, including steady states, stability and model solutions, require running Dynare with MATLAB or Octave.

For MATLAB integration, see the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

## Credits

1. Dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard and Mehrdad Samadi. That repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation, tag and extraction ideas are informed by the Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
3. The bundled agent skill is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

[GPL-3.0-or-later](LICENSE). The bundled use-dynare skill retains its upstream [MIT license](https://github.com/EconSolider/dynare-copilot/blob/main/LICENSE).
