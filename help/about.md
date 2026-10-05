# About Dygnosis

Dygnosis provides language support for Dynare, in an editor and for agents
through MCP. Its Rust engine shares parsing, diagnostics and references between LSP
and MCP. It stops before MATLAB or Octave and does not run the official Dynare
preprocessor.

## What the results mean

The official Dynare 7.2 preprocessor is the language authority. Dygnosis shows
the model you edit before Dynare transforms equations for numerical work.
Macro and include expansion can contribute to that model. Counts, timing,
parameter hints, project coverage and structural comparisons are source facts;
they do not establish existence, uniqueness, stability or a solved model.

**Error** means an identified problem would make the official preprocessor
refuse before MATLAB, and Dygnosis can locate it in the written source.
**Warning** means something looks wrong without that refusal; some warnings
are additional checks. **Information** asks you to confirm intent. The
[check index](reference.md) distinguishes shared, added and skipped checks.
A skipped entry is documented but is not emitted.

Language coverage is incomplete. Unsupported macro state, unavailable source
mapping and problems requiring later numerical computation can withhold facts
or checks. A clean editor is not proof that official Dynare will accept or
solve the model. Run official Dynare with MATLAB or Octave for those results.

## Credits and license

Dygnosis v0.1.0 began as a fork and Rust rewrite of
[LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony
Diercks, Philip Howard and Mehrdad Samadi. That repository accompanies
*LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted
Macroeconomic Modeling*. Equation, tag and extraction ideas are informed by the
Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
The bundled workflow skill is adapted from
[EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot) and
retains its upstream [MIT license](https://github.com/EconSolider/dynare-copilot/blob/main/LICENSE).
Dygnosis uses
[GPL-3.0-or-later](https://github.com/naivej/dygnosis/blob/main/LICENSE).
Packages include dependency license files and `THIRD-PARTY-NOTICES.json`.
`SOURCE.json` records the source commit, native target, lockfile hashes,
binary checksum and build toolchain. GitHub binary archives carry the same
engine and notices as the matching extension package.

For numerical work in VS Code, see the
[MATLAB extension](https://github.com/mathworks/MATLAB-extension-for-vscode).
Agents can use the [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

[Official Dynare manual](https://www.dynare.org/manual/) supplies the language
and numerical-work reference. [Dygnosis source](https://github.com/naivej/dygnosis)
and [releases](https://github.com/naivej/dygnosis/releases) provide the product
source and standalone MCP binaries.
