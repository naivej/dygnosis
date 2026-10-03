# Native MCP tools in VS Code

The extension registers a **Dygnosis** MCP server using its bundled engine.
Registration does not require an open model or a running language server.
It provides the same fifteen analysis/editing tools as the standalone MCP
transport. Model inputs and results remain engine data.

Use the Command Palette's **MCP: List Servers** to manage the server and view
its Output. In a chat that supports MCP, the tool picker controls which tools
are available. VS Code manages its own server/tool approval and trust controls;
see its [MCP guide](https://code.visualstudio.com/docs/agent-customization/mcp-servers).

An absolute user/machine `dynare.serverPath` override applies to both the
language client and native MCP provider. Use the matching native engine for
the workspace host. A remote window runs the workspace extension and its
engine there. If an override is incompatible, the Dygnosis Output and Settings
provide the recovery path; use a compatible engine or the bundle.

For another agent that reads project-local configuration, run **Dygnosis: Set
up MCP for this project** and follow [project-mcp.md](project-mcp.md). That
command preserves other servers and uses a managed copy or your explicit
override. It is a separate setup from native VS Code MCP registration.
