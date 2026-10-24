![Dygnosis](media/logo_s.png)

Dygnosis adds Dynare language support to VS Code. Install it from the [VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=CoconutWater.dygnosis) to start editing models.

- Edit with live diagnostics, hover, completion, formatting, quick fixes and commented shock templates. Syntax help covers Dynare commands, options, macros and PAC/VAR forms.
- Follow names with **Go to Definition**, **Find All References** and **Rename symbol** across known includes. Browse **Outline**, fold each `@#if` arm separately and open linked model files.
- Choose an **analysis configuration** for a model root: keep **Source**, set macro inputs, preview another `@#if` branch or select equations by tag. Status bar counts follow the selection and stay separate from inactive written source.
- Inspect equations, includes and **Expand macros** previews, then **Go to written source**. Jump to any counted equation from the Command Palette.
- Compare models in **Show mod changes**: Git revisions, another `.mod` file or two analysis configurations for the same root.
- See every Dynare model in the workspace, with checks that update as you edit.
- Run **Dygnosis: Open Help** for topics, settings and diagnostic codes.

It also provides [MCP tools](https://github.com/naivej/dygnosis/blob/main/help/agents.md) and a [use-dynare skill](https://github.com/naivej/dygnosis/blob/main/.agents/skills/use-dynare/SKILL.md) so agents can inspect, check, compare and propose edits with the same analysis choices.

Editing and source analysis work without a Dynare installation. Run official Dynare in MATLAB or Octave for steady states, stability checks and model solutions. For MATLAB integration, see the [MATLAB extension for VS Code](https://github.com/mathworks/MATLAB-extension-for-vscode) and [MATLAB agentic toolkit](https://github.com/matlab/matlab-agentic-toolkit).

**Compatibility**: Dynare 7.2

## Credits

1. Dygnosis v0.1.0 is a fork and rewrite of [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard and Mehrdad Samadi. That repository accompanies the working paper *LLMacro: A Language Server for Dynare — Structured Context for AI-Assisted Macroeconomic Modeling*.
2. Equation, tag and extraction ideas are informed by the Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder).
3. The bundled agent skill is adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).

## License

Dygnosis is licensed under [GPL-3.0-or-later](https://github.com/naivej/dygnosis/blob/main/LICENSE). The use-dynare skill is licensed under [MIT](https://github.com/naivej/dygnosis/blob/main/.agents/skills/use-dynare/LICENSE).
