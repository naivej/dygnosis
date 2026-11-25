# About and support

Supports Dynare 7.2 and VS Code 1.102.0 or later.
[VS Code Marketplace](https://marketplace.visualstudio.com/items?itemName=CoconutWater.dygnosis) ·
[GitHub releases and standalone bundles](https://github.com/naivej/dygnosis/releases)

## Support

[Dygnosis: Open Settings](action:settings) · [Show Output](action:output) · [Restart language server](action:restart)

Clear [Dynare: Server Path](settings:dynare.serverPath) to use the bundled
engine. Help works offline even if the language server cannot start.

## Report a problem

Open a [GitHub issue](https://github.com/naivej/dygnosis/issues) with a minimal
`.mod` example, the expected and actual behavior, the extension and engine
versions, your OS, and whether you use a remote workspace. Include relevant
lines from the **Dygnosis** channel in **Output**.

## Credits

Dygnosis uses the [GPL-3.0-or-later license](https://github.com/naivej/dygnosis/blob/main/LICENSE).
The `use-dynare` skill uses the [MIT license](https://github.com/naivej/dygnosis/blob/main/.agents/skills/use-dynare/LICENSE).

- [LLMacro-Dynare-LSP](https://github.com/pdwhoward/LLMacro-Dynare-LSP) by Anthony Diercks, Philip Howard and Mehrdad Samadi was the starting point.
- The Dynare Team's [modBuilder](https://git.dynare.org/Dynare/modBuilder) informed equation, tag and extraction features.
- The bundled skill was adapted from [EconSolider/dynare-copilot](https://github.com/EconSolider/dynare-copilot).
