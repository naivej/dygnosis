![Dygnosis](media/logo_s.png)

Dygnosis adds Dynare language support to VS Code with zero dependency. 

Install from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=CoconutWater.dygnosis) or download packages and standalone binaries from [GitHub Releases](https://github.com/naivej/dygnosis/releases), then run **Dygnosis: Open Help** in VS Code to explore.

## Highlights

- **Catch errors before running MATLAB**: Reports official Dynare preprocessor errors alongside static modeling checks (equation/variable balance, unassigned or unused parameters, missing steady-state or `initval` entries, `varobs` vs. shock counts, and timing or bound issues) with Quick Fixes and linked source locations.
- **Navigate across `@#include` trees**: Follow variables, parameters, `#` model-local variables, trends, and equations across root models and included files with **Go to Definition**, **Find All References**, **Show Call Hierarchy**, **Outline**, **Jump to equation...**, and safe cross-file **Rename symbol** (including `[endogenous=...]` and `[mcp=...]` tags).
- **Preview macros and test configurations**: Expand `@#include` files, `@#for` loops, `@#if` branches, and `@{...}` substitutions side-by-side with jumps back to the written line. Switch **Analysis Configurations** to test `-D` macro inputs, override `@#define` values, preview conditional branches, or filter equations with `include_eqs` and `exclude_eqs` without editing your `.mod` file.
- **Read models at a glance**: See `long_name` and TeX labels (`$\beta$`) in hover, completion, and **Outline**, inline evaluated parameter hints (`beta = 1 / 1.04`), semantic highlighting by symbol role and timing (`predetermined` vs. `forward-looking`), and customizable block shading.
- **Write and refactor faster**: Get context-aware completion and option signature hints across Dynare commands and blocks (including heterogeneous-agent `heterogeneity_dimension`, semi-structural VAR/PAC, OccBin, Ramsey/discretionary policy, and method of moments), format documents, auto-generate commented `shocks` templates, and bulk-fill missing `[name='...']` tags or `long_name` labels.
- **Compare models structurally**: Diff your working model against Git commits, branches, another `.mod` file, or another analysis configuration in **Dygnosis Changes**—grouped by equations, parameters, priors, and shocks rather than raw line diffs.
- **Audit workspace models**: Monitor check status across all saved `.mod` and `.dyn` root models in your project from Explorer's **Dynare workspace checks** view.
- **Work with AI coding agents**: Connect GitHub Copilot and other MCP clients to Dygnosis's 12 [MCP tools](https://github.com/naivej/dygnosis/blob/main/help/agents.md) (model inspection, equation extraction, structural diff, safe rename, formatting, and diagnostics) and install the [`use-dynare` agent skill](https://github.com/naivej/dygnosis/blob/main/.agents/skills/use-dynare/SKILL.md) via `npx skills add naivej/dygnosis`.

## Running Models

Dygnosis checks and analyzes model source files statically. Run official Dynare in MATLAB or Octave to compute steady states, verify Blanchard-Kahn conditions, and execute simulations or estimation. For MATLAB integration in VS Code, see the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

**Compatibility**: Dynare 7.2

## Credits

1. Dygnosis started as a fork and full rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard and Mehrdad Samadi (*LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*).
2. Equation, tag and extraction features are informed by the Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
3. The bundled agent skill is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

Dygnosis is licensed under [GPL-3.0-or-later](https://github.com/naivej/dygnosis/blob/main/LICENSE). The `use-dynare` skill is licensed under [MIT](https://github.com/naivej/dygnosis/blob/main/.agents/skills/use-dynare/LICENSE).
