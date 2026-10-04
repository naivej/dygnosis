# Dygnosis packages

The 0.11.5 candidate includes the Dygnosis engine for its workspace host. It
does not download the engine when activated. Its analysis requires no separate
Rust, Python, MATLAB or Dynare installation. Dygnosis reads models and stops
before MATLAB computation; it does not run the official Dynare preprocessor.

Earlier candidates passed all six installed VS Code 1.102.0/1.140.0 and unpacked
standalone checks, including the collector, in
[native CI](https://github.com/naivej/dygnosis/actions/runs/37086177953).
Release review and publication remain pending; final release versions require
a fresh matching package matrix.

| Package target | Native verification host | Runtime evidence |
|---|---|---|
| `win32-x64` | Windows Server 2022, build 20348 | Native launch passed; C runtime linked statically. Running-image managed updates passed locally; final packaged/client upgrade checks remain pending. |
| `win32-arm64` | Windows 11 ARM, build 26200 | Native ARM64 launch passed |
| `darwin-x64` | macOS 15 Intel, Darwin 24.6 | Native launch passed; deployment target 13.0 is not a tested minimum |
| `darwin-arm64` | macOS 15 ARM, Darwin 24.6 | Native ARM64 launch passed |
| `linux-x64` | Ubuntu 22.04 x64 | Native launch passed; highest required glibc symbol version is 2.34 |
| `linux-arm64` | Ubuntu 22.04 ARM | Native ARM64 launch passed; highest required glibc symbol version is 2.34 |

The extension requires VS Code 1.102.0 or later. It runs in the workspace
extension host: WSL, SSH and container workspaces need the package matching
that host. A Windows desktop with a Linux remote workspace is a required
release check. Browser-only, Alpine and 32-bit hosts are outside this matrix.
VS Code has its own [OS and Linux library requirements](https://code.visualstudio.com/docs/supporting/requirements);
the packaged engine's measured requirements can be higher. Linux packages
also require their recorded system `libgcc_s`, `libm`, and loader. The table
records tested candidate hosts and measured symbols rather than untested older
OS support. Remote placement and final native MCP routing remain release gates.

A version tag runs the compatibility workflow, then publishes the six tested
engine binaries and the six VSIX files to [GitHub Releases](https://github.com/naivej/dygnosis/releases).
The asset names are `dygnosis-<target>`, `dygnosis-<target>.exe` on Windows,
`dygnosis-<version>-<target>.vsix`, and `SHA256SUMS`. Marketplace publication
of the VSIX files is local. The
0.11.5 candidate is not published. Updates replace the
client and its engine together. An explicit absolute `dynare.serverPath` in
user/machine settings selects another executable for both LSP and the VS Code
MCP provider. Read the [MCP guide](vscode-mcp.md) for executable override recovery.

For standalone CLI or MCP use, download the binary for your host from the GitHub
Release and verify its SHA-256 against `SHA256SUMS`. Keep the executable in a
persistent directory. The tag's source tree has the GPL license, dependency
notices, and `SOURCE.json` for that commit.

```sh
/absolute/path/dygnosis-linux-x64 --version
/absolute/path/dygnosis-linux-x64 check /path/to/model.mod
```

On Windows the asset is `dygnosis-win32-x64.exe` or `dygnosis-win32-arm64.exe`.
Use that absolute path. Configure an MCP client to start that
executable with the single argument `mcp`; LSP clients start it without
arguments. Passing executable and argument fields separately keeps paths with
spaces usable. Standalone packages need no Node.js installation.

Dygnosis uses [GPL-3.0-or-later](../LICENSE). The product source link, Cargo and
npm lock hashes, binary/logo hashes, and build toolchain are in `SOURCE.json`.
`THIRD-PARTY-NOTICES.json` lists dependency versions and included license files.
The VSIX preserves its JavaScript runtime dependencies. The standalone archive
uses the identical native engine and carries the same license notices.
