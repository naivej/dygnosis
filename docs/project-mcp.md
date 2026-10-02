# Project MCP setup

Run **Dygnosis: Set up MCP for this project** in a trusted workspace. You can
run it without opening a Dynare model. In a window with several folders, choose
the project. Dygnosis uses its containing repository root when VS Code's Git
extension exposes one; otherwise it uses the selected folder. The command
shows the `.mcp.json` destination and opens the result for review.

The command writes the [Claude Code project format](https://code.claude.com/docs/en/mcp#project-scope):

```json
{
  "mcpServers": {
    "dygnosis": {
      "type": "stdio",
      "command": "<absolute executable path on this host>",
      "args": ["mcp"]
    }
  }
}
```

Other servers, unrelated keys and their original text stay intact. An identical
Dygnosis entry needs no change. An existing different entry or a dirty editor
buffer opens a read-only Before/Proposed diff. Choose **Save reviewed setup**
to save that result, or cancel to leave the configuration unchanged. Setup
uses the unsaved buffer as its source and checks its version and the disk
contents before saving. If either changes, review the current file and run
setup again. Invalid JSON, comments, trailing commas, duplicate keys, a
non-object `mcpServers`, and symlinked config files must be resolved before
setup; the command does not repair them.

Start Claude Code in that project and use its MCP controls to approve and
connect the server. Claude Code owns those approval and trust prompts. After
an executable update, restart or reconnect its Dygnosis MCP server to load the
new version.

## Executable updates

With the included engine, setup copies verified package bytes to
`bin/dygnosis.exe` on Windows or `bin/dygnosis` on macOS/Linux under the
extension's [API-provided global storage](https://code.visualstudio.com/api/references/vscode-api#ExtensionContext).
That path is independent of the installed extension version. The copy retains
the package's version, native target, SHA-256 checksum and source provenance.
It is created by setup, then checked for updates whenever the extension
activates. Update staging, version comparison and replacement share a
filesystem lock across windows; an older window cannot downgrade the copy.
An update with different bytes for the same version is refused for review.

Windows can prevent replacement while another agent runs the executable.
Dygnosis preserves the working version, reports the pending version, and
retries up to five times at 15-second intervals. It retries again on the next
activation or setup invocation. Stop the other agent's Dygnosis MCP server to
release the file, then restart it after the update completes. Running servers
continue using their loaded version. The `.mcp.json` path stays the same.
An interrupted binary/metadata commit is recovered from the verified pending
record before a later update proceeds.

A live window's lock is never removed automatically. A dead process's lock is
recovered under a separate guard. Malformed locks or a guard left by an
interruption during lock recovery time out safely; close all Dygnosis windows
and remove only the named `.dygnosis.lock`/`.recovery` files in extension
storage, or the `.mcp.json.dygnosis.lock`/`.recovery` files beside that project's
config, before retrying. Keep the executable and provenance records intact.

If you set the user/machine `dynare.serverPath`, setup uses that absolute path
after checking the executable's MCP compatibility. Its updates are
user-managed. Overrides are never copied into managed storage or overwritten.
Changing the setting changes LSP and VS Code's MCP provider; an existing
project config changes only when you run setup and review its proposed edit.

## Host and storage limits

The generated path belongs to the machine where the extension runs. A remote
workspace uses the remote extension host's copy. Run setup on each host or
checkout that needs its own path. An agent running on another host needs its
own matching local binary; an absolute path in a committed config is not
portable across machines.

The managed path lasts while that extension's storage remains present.
Uninstalling the extension, switching to a profile with separate storage, or
deleting the storage can require setup again. A
[standalone release binary](https://github.com/naivej/dygnosis/releases) is an
independent installation route for users who want to maintain their own path.
Dygnosis does not install a system command or change `PATH`.

The extension generates only `.mcp.json` and leaves agent-global settings
untouched. For another agent format, ask that agent:

> Port this repo's `.mcp.json` to your project-local MCP configuration. Keep
> dygnosis's command and arguments, preserve other server entries, and leave
> global settings untouched.
