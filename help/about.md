# About Dygnosis

Dygnosis provides Dynare language support in editors and through MCP agent
tools. Language support follows **Dynare 7.2**.

## Analysis limits

Dygnosis analyzes model source before numerical work. It does not run the
official Dynare preprocessor, MATLAB or Octave. Counts, timing, value hints and
model comparisons do not establish a steady state, stability or a solution.

Some language checks and macro cases are outside coverage. Incomplete analysis
is reported and affected results are withheld. A clean editor does not prove
that official Dynare will accept or solve the model.

The [official Dynare manual](https://www.dynare.org/manual/) describes Dynare's
language and numerical features.

## Credits and license

Dygnosis v0.1.0 began as a fork and Rust rewrite of
[LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony
Diercks, Philip Howard and Mehrdad Samadi. That repository accompanies
*LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted
Macroeconomic Modeling*. Equation, tag and extraction ideas are informed by the
Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
The bundled use-dynare skill is adapted from
[EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot) and
retains its upstream [MIT license](https://github.com/EconSolider/dynare-copilot/blob/main/LICENSE).

Dygnosis uses
[GPL-3.0-or-later](https://github.com/naivej/dygnosis/blob/main/LICENSE).
Packages include dependency licenses, third-party notices and source records.
[Source](https://github.com/naivej/dygnosis) and
[releases](https://github.com/naivej/dygnosis/releases) are on GitHub.
