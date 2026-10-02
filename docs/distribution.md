# Dygnosis packages

The 0.11.1 extension includes the Dygnosis engine for its workspace host. It
does not download the engine when activated. Its analysis requires no separate
Rust, Python, MATLAB or Dynare installation. Dygnosis reads models and stops
before MATLAB computation; it does not run the official Dynare preprocessor.

Packaging preparation is in progress. The following targets are candidates
until their installed-package checks and release review pass. A successful
compilation alone does not establish support.

| Package target | Native verification host | Runtime evidence |
|---|---|---|
| `win32-x64` | Windows Server 2022 CI; Windows desktop manual check | Pending; release builds link the C runtime statically |
| `win32-arm64` | Windows 11 ARM CI | Pending; native ARM64 process required |
| `darwin-x64` | macOS 15 Intel CI | Pending; deployment target 13.0 is a build setting, not a tested minimum |
| `darwin-arm64` | macOS 15 ARM CI | Pending; native ARM64 process required |
| `linux-x64` | Ubuntu 22.04 x64 CI | Pending; archive records ELF version requirements and `ldd` output |
| `linux-arm64` | Ubuntu 22.04 ARM CI | Pending; archive records ELF version requirements and `ldd` output |

The extension requires VS Code 1.102.0 or later. It runs in the workspace
extension host: WSL, SSH and container workspaces need the package matching
that host. A Windows desktop with a Linux remote workspace is a required
release check. Browser-only, Alpine and 32-bit hosts are outside this matrix.
VS Code has its own [OS and Linux library requirements](https://code.visualstudio.com/docs/supporting/requirements);
the packaged engine's measured requirements can be higher. Record the tested
OS versions and measured library floor here before advertising a target.

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
