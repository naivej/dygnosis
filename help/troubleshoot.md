# Support and problem reports

**Dygnosis Help** works offline even before a model opens or when the language
server cannot start. Press `F1` and type `Dygnosis` to run these support
commands from the Command Palette:

| Command or setting | Use |
|---|---|
| [Dygnosis: Show Output](action:output) | Opens the **Output** panel. Select **Dygnosis** from the channel dropdown to inspect startup, analysis and error logs. |
| [Dygnosis: Restart language server](action:restart) | Restarts the Dygnosis analysis engine. |
| **Dygnosis: Open Settings** | Opens VS Code's Settings editor filtered to `@ext:CoconutWater.dygnosis`. |
| [Dynare: Server Path](settings:dynare.serverPath) | Selects a custom `dygnosis` executable in user/machine settings. Leave it empty to use the bundled binary. |

## Report a problem

Open an issue on the [GitHub issue
tracker](https://github.com/naivej/dygnosis/issues) and include the extension
and engine versions, your OS (and whether you are using a Remote workspace),
relevant lines from **Dygnosis** in **Output**, and a minimal `.mod` example
showing the expected and actual behavior.
