# Troubleshooting

[Dygnosis Output](action:output) records startup and failure details. Help stays
available when the engine cannot run.

| Problem | Recovery |
|---|---|
| Engine does not start | Reinstall the extension package for the workspace host, or check the [server path](settings:dynare.serverPath) override. It must name a Dygnosis executable. |
| Feature unavailable with an older engine | Clear the override to use the bundled engine, then [restart the language server](action:restart). |
| Include has no model owner | Open its root `.mod` or `.dyn`. **Choose model owner** selects between known owners. |
| Incomplete analysis | Resolve the reported include, parse or macro problem. Counts and source jumps can be unavailable until then. |
| Preview or comparison out of date | Refresh it to use current inputs. |
| Hint, color or CodeLens missing | Check the feature setting and native VS Code visibility controls. |
| No project roots | Background discovery needs a file-backed folder with saved `.mod` roots. |
| Project check failed | The project view and Output identify read or discovery failures. **Recheck project** retries. |
| MCP setup rejected | Setup needs a trusted file-backed workspace and valid `.mcp.json`; duplicate keys, unsupported file structure and symlinks require correction. |
| MCP update pending on Windows | Stop the other client's Dygnosis server so its executable can be replaced, then reconnect. |

Virtual filesystems have syntax coloring only. Untitled files have no disk
base for includes. A file treated as plain text needs Dynare language mode.

## Report a problem

The [issue tracker](https://github.com/naivej/dygnosis/issues) accepts reports.
Useful details are the extension and selected engine versions, OS, remote
workspace status, relevant Output and a small shareable model that shows the
expected and actual result.
