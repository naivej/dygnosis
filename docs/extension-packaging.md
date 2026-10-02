# Building and verifying extension packages

Run commands from `editors/vscode`. Node.js 24, Rust 1.98.0, Cargo and the
platform's native build tools are needed by contributors. Installed users do
not need them. Install the locked JavaScript dependencies with
`npm ci --ignore-scripts`; packaging does not need the optional VSCE signing
helper's install script.

`npm run package:target -- --target win32-x64 --candidate` builds a native
release engine and prepares a VSIX plus standalone archive under
`dist/win32-x64`. Substitute a target from the six-target matrix in
[distribution](distribution.md). Packaging checks the host's real architecture,
matching Cargo/client/binary versions, product logo, actual binary architecture,
runtime dependencies, included license text and Unix executable permissions.
Windows release builds use a static C runtime. Candidate source records reveal
uncommitted changes and cannot pass the publication check.

For a local development engine, append `--binary /absolute/path/to/dygnosis`
in candidate mode. Such a package is useful for integrated checks but records
that its binary was provided, with no claim that it was a tagged release build.

`npm run verify:package -- --target win32-x64 --vscode 1.102.0` checks the actual
package and standalone archive, installs the VSIX into a fresh isolated profile,
then launches the installed extension. Repeat with `--vscode stable`. An existing
portable host can be selected with `--executable /absolute/path/to/Code.exe` and
an explicit `--vscode` version. Tests use profile, extension, model, unpack and
override paths containing spaces; they never write the normal user profile.
Windows hosts start hidden. Linux desktop hosts need a display such as
`xvfb-run -a`.

Evidence separates installed LSP/Problems/navigation, bundled stdio MCP,
override launches and unpacked standalone launches. Native VS Code MCP
discovery, trust and tool routing require Ext-launch evidence. The direct stdio
probe does not claim to satisfy that gate. Manual release evidence also covers
an extension upgrade, Windows desktop with a Linux remote host, and the tested
runtime floor. Minimum/current host JSON files are fresh per run and include the
exact artifact hashes, target, version and source commit.

The [Extension artifacts workflow](../.github/workflows/extension-build.yml)
runs those checks natively on all six targets with VS Code 1.102.0 and current
stable. It records runtime libraries and collects exact tested artifacts after
the entire matrix passes. Packaging needs network access for locked crates/npm
dependencies, VS Code test hosts, and the exact upstream MCP SDK license omitted
from the crate archives. That license source is pinned to the crate's own
`.cargo_vcs_info.json` commit; an unavailable license fails packaging.

For release builds, freeze the reviewed product commit, update Cargo/client and
release notes to the same version, then use the matching existing tag:
`npm run package:target -- --target win32-x64 --tag vVERSION`. Release mode
requires a clean tagged checkout and builds its own engine from it. Every
artifact retains `SOURCE.json`, checksums and license notices. The provenance
records the product license hash and its exact package paths: VSCE writes
`extension/LICENSE.txt`, while the standalone archive retains `LICENSE`. Later release
versions reuse this pipeline and repeat installed checks.

Tags through v0.11.0 retain the legacy Rust notes-only route. Starting at
v0.11.1, [Release](../.github/workflows/release.yml) verifies the full package
matrix and leaves publication to the separate
[Extension publication workflow](../.github/workflows/extension-publish.yml).
Publication downloads the successful run's files, checks its repository,
workflow and exact tagged commit, and verifies every artifact checksum. It
creates a GitHub draft, uploads the tested VSIX/standalone files, publishes those
same VSIX paths to Marketplace, and publishes the draft after the uploads pass.
It never rebuilds a package while publishing.

Before dispatching publication, commit a reviewed `release-gates/vVERSION.json`
on the dispatch branch after verification. It names the tagged artifact commit
and version, and records `passed: true` plus an evidence location for each of
`publisher`, `native_vscode_mcp`, `extension_upgrade`, `windows_linux_remote`,
`runtime_requirements`, and `client_review`. Gates are read from the dispatch
commit, independently of the frozen artifact tag, so recording evidence does
not change the tested source. Missing gates prevent publication.

Publisher `dygnosis` is provisional. Confirm ownership in
[Marketplace publisher management](https://marketplace.visualstudio.com/manage),
then configure the reviewed credential in the protected `marketplace` GitHub
environment. The current workflow accepts `VSCE_PAT`; credential setup is still
open. Microsoft's [publishing guide](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#secure-automated-publishing-to-visual-studio-marketplace)
recommends Entra authentication and describes the December 2026 global-PAT
retirement. Select and verify the owner's credential route before publication.
No credential is required by the build/verification workflow. A partial
Marketplace upload leaves the GitHub draft unpublished; inspect and reconcile
that draft and uploaded platform versions before retrying publication.
