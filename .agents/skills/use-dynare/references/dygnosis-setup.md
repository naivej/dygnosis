# Dygnosis setup

Read this when the Dygnosis tools (names start with `dynare_`) are not callable in this session, or
when they fail. Setup ends only when a real model query succeeds.

```text
Discover the Dygnosis tools
  -> callable: use them; do not reinstall
  -> configured but unavailable: inspect the host status and repair the connection
  -> absent: find or install the executable, then configure this host
  -> verify with a model query, then continue the task
```

Do the setup within the user's authorization and the host's permissions. Downloading a file and
changing a host configuration need the user's consent; reuse consent already given, and ask otherwise.

## 1. Discover

1. Look at the tools that this session exposes. If `dynare_*` tools are listed and a call works, use
   them. Do not reinstall.
2. If the tools are listed but calls fail, or the host shows the server as stopped, disabled, failed or
   waiting for approval, the server is **configured but unavailable**. Go to step 3 and step 4.
3. If no `dynare_*` tool is listed, look for a `dygnosis` entry in the host's MCP configuration
   (table in step 3). An entry that exists is **configured but unavailable**. No entry means
   **absent**: go to step 2.

Common causes of "configured but unavailable": a wrong or relative executable path, a missing `mcp`
argument, an executable for another OS or architecture, a server that the user has not approved, a
project that the host does not trust, and a configuration that needs a reload or a new session.

## 2. Get an executable

Use the first route that works.

1. **Reuse a compatible executable.** Candidates: a path that the user gives, `dygnosis` on `PATH`, the
   command of an existing `.mcp.json` entry, or the copy that the VS Code extension manages. The
   extension command **Dygnosis: Set up MCP for this project** creates that copy at `bin/dygnosis.exe`
   (Windows) or `bin/dygnosis` (macOS, Linux) in the extension's global storage. Run
   `<path> --version`. The executable must run on the machine that runs the agent.
2. **Download a release.** Open [GitHub Releases](https://github.com/naivej/dygnosis/releases). Choose
   the asset for the OS and architecture of the machine that runs the agent. Release targets:
   `win32-x64`, `win32-arm64`, `darwin-x64`, `darwin-arm64`, `linux-x64`, `linux-arm64`.
   - Read the actual asset names from the release (the page, or
     `gh release view <tag> -R naivej/dygnosis --json assets`). Do not build a download URL from a
     name pattern.
   - Verify the SHA-256 of the download against `SHA256SUMS` from the same release.
   - Extract to a persistent folder, not a temporary or build folder. Keep the license and provenance
     files. On macOS and Linux, make the file executable.
   - Run `<path> --version`.
3. **No route.** Some releases publish no binaries. If no asset matches and no executable exists,
   stop the setup and report the unavailable state (step 6). Do not build from source, and do not use a
   file from another release page or site.

## 3. Configure the host

Rules for every host:

- The command is the **absolute path** of the executable. `mcp` is a **separate argument**.
- Prefer project scope. Keep the other servers and unrelated keys in the file.
- The path belongs to the machine that runs the agent. A remote agent needs its own executable.
- In JSON, escape Windows backslashes (`"C:\\tools\\dygnosis\\dygnosis.exe"`) or use forward slashes.

| Host | Configuration | Check |
|---|---|---|
| VS Code with the Dygnosis extension | No file needed: the extension registers a native **Dygnosis** server. Start it from **MCP: List Servers**. | **MCP: List Servers**, the server's Output |
| Claude Code | `claude mcp add --scope project dygnosis -- <absolute-path> mcp` writes `.mcp.json` at the project root. In VS Code, **Dygnosis: Set up MCP for this project** writes the same format. | `claude mcp list`, `claude mcp get dygnosis`, `/mcp` |
| Codex | Project `.codex/config.toml` (trusted projects only) or `~/.codex/config.toml`, below. Or `codex mcp add dygnosis -- <absolute-path> mcp`. | `codex mcp list`, `/mcp` |
| Cursor | Project `.cursor/mcp.json` or `~/.cursor/mcp.json`, below. | Output panel, channel "MCP Logs"; the server toggle in the settings |
| Other hosts | Use the host's own format with the same command and argument. | The host's MCP status |

`.mcp.json` (Claude Code) and `.cursor/mcp.json` (Cursor):

```json
{
  "mcpServers": {
    "dygnosis": {
      "type": "stdio",
      "command": "/absolute/path/to/dygnosis",
      "args": ["mcp"]
    }
  }
}
```

`.codex/config.toml` (Codex). On Windows, write the path as a single-quoted TOML string, which keeps
backslashes: `command = 'C:\tools\dygnosis\dygnosis.exe'`.

```toml
[mcp_servers.dygnosis]
command = "/absolute/path/to/dygnosis"
args = ["mcp"]
```

The `.mcp.json` format is Claude Code's. It is not a format for every client.

## 4. Trust, approval and reload

Some steps need the user:

- **Claude Code:** the user approves a project server in an interactive session (status
  "Pending approval"). Approvals do not apply in an untrusted folder. `claude mcp reset-project-choices`
  resets them.
- **Codex:** project configuration loads only in a trusted project. Start a new session after a change.
- **Cursor:** the user enables the server and approves tool calls. A reload can be necessary.
- **VS Code:** workspace trust and the host's tool approval apply.

If a step needs the user, give the exact action (for example "run `claude` in this folder and approve
the `dygnosis` server"), mark the connection as **not verified**, and continue with work that does not
need the tools. Do not reinstall or rewrite the configuration to get around the step.

## 5. Verify

After you connect or reconnect:

1. List the tools whose names start with `dynare_`.
2. Call `dynare_diagnose` with this text as `file_content`:

   ```text
   var y;
   varexo u;
   parameters rho;
   rho = 0.9;
   model;
   y = rho*y(-1) + u + zz;
   end;
   ```

   Expect an Error E020 for the undeclared identifier `zz`, and some Information notes (I050, I208,
   I209).
3. Record the host, the executable path and the version (`<path> --version`).

A configuration file or a successful `--version` alone does not prove MCP access. After you replace the
executable, reconnect the server.

## 6. Unavailable state

If the tools stay unavailable:

- Tell the user what is missing: the executable, the configuration, an approval, a trust decision, or a
  reload.
- Continue the work that does not need Dygnosis: reading and writing the model, and Dynare runs when
  MATLAB is connected ([MATLAB Agentic Toolkit](https://github.com/matlab/matlab-agentic-toolkit)).
- Label the evidence: "Dygnosis checks were not run". Editor diagnostics from the VS Code extension are
  Dygnosis results; name them as editor diagnostics.
- No command-line program replaces the MCP tools. `dynare_workspace_diagnose` also needs MCP.

## 7. Install this skill

Copy the **whole skill folder** — `SKILL.md`, `LICENSE` and `references/` with its catalogs, example
models, archive and scripts — from a Dygnosis checkout (folder `.agents/skills/use-dynare/`) into
the host's skill location. The folder name must equal the `name` in the `SKILL.md` frontmatter.

| Host | Project location | Personal location |
|---|---|---|
| Claude Code | `.claude/skills/use-dynare/` | `~/.claude/skills/use-dynare/` |
| Codex | `.agents/skills/use-dynare/` (the current folder up to the repository root) | `~/.agents/skills/use-dynare/` |
| Cursor | `.agents/skills/` or `.cursor/skills/` | `~/.agents/skills/` or `~/.cursor/skills/` |

Claude Code does not read `.agents/skills`. Cursor also reads `.claude/skills` and `.codex/skills`.
Restart Codex if a new skill does not appear. Installing the VS Code extension or registering the MCP
server does not install this skill.

## More help

- [MCP without VS Code](https://github.com/naivej/dygnosis/blob/main/help/get-started.md#mcp-without-vs-code)
- [Connect an agent](https://github.com/naivej/dygnosis/blob/main/help/agents.md) (VS Code native MCP and
  project setup)
- [Troubleshooting](https://github.com/naivej/dygnosis/blob/main/help/troubleshoot.md) (executable
  override, workspace trust, startup)
