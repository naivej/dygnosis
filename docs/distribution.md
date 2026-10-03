# Dygnosis packages

The 0.11 client preparation includes the Dygnosis engine for its workspace host. It
does not download the engine when activated. Its analysis requires no separate
Rust, Python, MATLAB or Dynare installation. Dygnosis reads models and stops
before MATLAB computation; it does not run the official Dynare preprocessor.

All six candidate targets passed installed VS Code 1.102.0/1.140.0 and unpacked
standalone checks, including the final collector, in
[native CI](https://github.com/naivej/dygnosis/actions/runs/37068721595).
Release review and publication remain pending; final release versions require
a fresh matching package matrix.

| Package target | Native verification host | Runtime evidence |
|---|---|---|
| `win32-x64` | Windows Server 2022, build 20348 | Native launch passed; C runtime linked statically. Windows desktop upgrade check remains pending. |
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

Each release offers target-specific VSIX files and standalone `.tar.gz` archives
on its [GitHub Release](https://github.com/naivej/dygnosis/releases). The VSIX
files on GitHub and Marketplace use the same tested bytes. Updates replace the
client and its engine together. An explicit absolute `dynare.serverPath` in
user/machine settings selects another executable for both LSP and the VS Code
MCP provider. Read the [settings guide](editor-settings.md) for recovery controls.

For standalone CLI or MCP use, choose the archive matching your host and verify
its SHA-256 against the release evidence. Unpack it into a persistent directory;
the archive includes the executable, GPL license, dependency notices, and
`SOURCE.json` with the exact product source commit and source archive link.
Keep those files together when redistributing the binary.

```sh
tar -xzf dygnosis-VERSION-linux-x64.tar.gz
/absolute/path/dygnosis-VERSION-linux-x64/dygnosis --version
/absolute/path/dygnosis-VERSION-linux-x64/dygnosis check /path/to/model.mod
```

Windows can unpack `.tar.gz` using `tar` or an archive application. Use the
absolute unpacked `dygnosis.exe` path. Configure an MCP client to start that
executable with the single argument `mcp`; LSP clients start it without
arguments. Passing executable and argument fields separately keeps paths with
spaces usable. Standalone packages need no Node.js installation.

Dygnosis uses [GPL-3.0-or-later](../LICENSE). The product source link, Cargo and
npm lock hashes, binary/logo hashes, and build toolchain are in `SOURCE.json`.
`THIRD-PARTY-NOTICES.json` lists dependency versions and included license files.
The VSIX preserves its JavaScript runtime dependencies. The standalone archive
uses the identical native engine and carries the same license notices.
