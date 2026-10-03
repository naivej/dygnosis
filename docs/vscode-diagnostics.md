# Diagnostic controls in VS Code

Dygnosis diagnostics appear in the editor and Problems panel. Related locations appear beneath a diagnostic and open the earlier declaration or include site. The extension checks open model roots and their includes. It also checks unopened saved `.mod` roots in workspace folders by default; see [project checks](project-diagnostics.md).

Use the lightbulb or **Quick Fix** on a Dygnosis diagnostic to choose:

- **Ignore this check (code)** hides every diagnostic with that code in this window, including Errors, Warnings, and Information. It also hides fixes attached only to that check. Independent refactors stay available.
- **Explain this check (code)** opens the engine's explanation in a read-only Markdown preview. **Dygnosis: Explain a check** in the Command Palette asks for a diagnostic code.

Hiding is temporary display state. It does not edit your `.mod`, change the engine's checks, or affect diagnostics from other extensions. The command line and agent tools still receive all diagnostics. Later diagnostic updates respect the hidden list.

When checks are hidden, a separate status item shows the hidden-code count. Click it, or run **Dygnosis: Show a hidden check**, to restore one code. **Dygnosis: Show all hidden checks** restores all. These commands remain available when diagnostic quick-fix offers are disabled. Restoration replays the latest results or requests fresh results from the engine, preserving related locations and action context. The hidden list clears when workspace folders change, the window reloads, or the extension closes. Restarting the language server keeps the list.

In **Settings → Dygnosis: Diagnostics**, `dynare.diagnosticActions.ignore` and `dynare.diagnosticActions.explain` control the two quick-fix offers. Both default to `true`, apply per file or workspace folder, and take effect on the next action request without a restart. Reset them through the Settings editor. The hidden-code count is independent of the model-count status item; VS Code's native status-bar menu controls status-item visibility.

Explain documents use `dygnosis-explain:` and remain outside model analysis. They open in VS Code's native Markdown preview, with executable links and HTML removed. Explanations come from the selected engine; an older override without the Explain command shows an actionable compatibility message.
