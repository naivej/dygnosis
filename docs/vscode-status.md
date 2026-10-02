# Model counts and value hints in VS Code

With a `.mod` or `.dyn` active, Dygnosis shows aggregate endogenous, exogenous,
and equation counts in the status bar. Native icons identify the three numbers;
hover for their labels, timing counts, and separate heterogeneity dimensions.
Click the item to focus Outline. VS Code also makes status items available by
keyboard through **Focus Status Bar**.

The counts describe the written model before transformation. Each dimension
has its own count domain; its counts are shown separately in the tooltip.
Timing classes describe written leads and lags. These are model facts, not
solver results. Counts refresh while you edit. Updating, incomplete expansion,
or unavailable model information replaces the numbers until current facts are
available. The bar is hidden on `.inc` files and read-only previews.

Open **Dygnosis: Open Settings**, then use the Model overview controls:

| Setting | Default | Effect |
|---|---|---|
| `dynare.statusBar.enabled` | `true` | Show the active model's count bar. |
| `dynare.statusBar.counts` | `["endogenous", "exogenous", "equations"]` | Choose the displayed counts and their order. An empty list hides the bar. |

These settings apply immediately and can be overridden per workspace or
folder. They resolve against the displayed file. A `.mod` opened through an
include link keeps its chosen model owner; use **Dygnosis: Treat this file as a
model root** to change that context. Use the setting's gear menu and **Reset
Setting** to restore its default. Invalid stored values are handled safely and
explained in Dygnosis Output.

For example, to keep only equations:

```json
{
  "dynare.statusBar.counts": ["equations"]
}
```

Expression-value hints use the existing `dynare.parameterValueHints` control
under Value hints, on by default. They also respect VS Code's **Editor › Inlay
Hints: Enabled** (`editor.inlayHints.enabled`), including language-specific
overrides such as `[dynare]`. Turn off either control to hide the hints.

The binary supplies proven values for top-level assignments written as
expressions. Plain numbers are quiet, and expressions it cannot evaluate have
no value hint. These values come from arithmetic folding, not MATLAB execution.
Hiding hints or status counts leaves the engine's shared model information
available to other editor features and MCP.

See the [editor settings guide](editor-settings.md) for the shared settings and
LSP contracts.
