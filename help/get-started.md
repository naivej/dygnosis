# Dynare workflow

Dygnosis adds Dynare language support to VS Code for `.mod`, `.dyn` and `.inc`
files. Analysis works without a separate Dynare, MATLAB or Octave installation.

## Understand the model

[Model files and structure](model-and-includes.md) brings together includes,
related files, counts and timing. [Equation and name navigation](navigate-code.md)
connects declarations to their uses across the model.

## Write equations and experiment setup

[Editing support](edit-models.md) covers declarations, parameters, equations,
steady-state blocks, shocks and commands. [Labels, hints and colors](appearance.md)
make names, known values and block structure easier to read.

## Inspect and check

[Macro expansion](effective-model.md) shows the text produced by includes,
loops and substitutions. [Model checks](diagnostics.md) report problems while
you edit, with explanations and available fixes.

## Review changes and model variants

[Model comparison](structural-diff.md) shows changes against Git history or
another model. [Project checks](project-checks.md) cover unopened saved models
alongside the one being edited.

## Numerical work

Official Dynare with MATLAB or Octave performs steady-state calculations,
simulation, estimation and other numerical tasks. Dygnosis supports the source
work above; it does not run those tasks. [About Dygnosis](about.md)
describes what its results establish.

[Agent tools](agents.md) support the same source workflow through MCP.

**Dygnosis: Open Settings** contains the full configuration list. Feature pages
mention configuration where it changes that feature's behavior or appearance.
The [reference](reference.md) provides command, check and option lookup;
[troubleshooting](troubleshoot.md) covers extension problems.

## Availability

The extension requires **VS Code 1.102.0 or later**. Packages target Windows,
macOS and Linux on x64 and ARM64. Remote workspaces use the package on the
remote host. Virtual filesystems have syntax coloring only; browser-only,
Alpine and 32-bit hosts are outside the package targets.

Loose and untitled Dynare models receive editor analysis. A saved model gives
includes a disk location. An included file needs a known
[model owner](model-and-includes.md#model-roots-and-includes).

[Marketplace](https://marketplace.visualstudio.com/items?itemName=CoconutWater.dygnosis)
provides the extension; [GitHub Releases](https://github.com/naivej/dygnosis/releases)
provides candidate packages and standalone binaries when available.
[About Dygnosis](about.md) describes the scope of its checks and results.
