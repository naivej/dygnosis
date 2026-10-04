# Building and verifying extension packages

Run commands from `editors/vscode`. Node.js 24, Rust 1.98.0, Cargo and the
platform's native build tools are needed by contributors. Installed users do
not need them. Install the locked JavaScript dependencies with
`npm ci --ignore-scripts`; packaging does not need the optional VSCE signing
helper's install script.

`npm run package` builds a candidate VSIX and standalone archive for this
machine. The target name is `platform-arch` (`win32-x64` on 64-bit Windows).
Output is under `dist/<target>/`.

`npm run package:target -- --target win32-x64 --candidate` selects that target
explicitly. It builds a native release engine and prepares the same files under
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

A pushed `vMAJOR.MINOR.PATCH` tag runs [Release](../.github/workflows/release.yml),
which calls [CI](../.github/workflows/ci.yml). Each of the six native hosts runs
`cargo test`, builds the tagged package, records runtime libraries, and launches
the installed extension in VS Code 1.102.0 and current stable. The `verified`
job checks that the six results share one commit, version, publisher, and tag.
The Release workflow then creates the GitHub Release from the `## vX.Y.Z`
changelog section and uploads the six tested engine binaries, the six VSIX
files, and `SHA256SUMS`. The `verified-packages` artifact also keeps the
standalone archives and verification reports. Format, Clippy, `npm run check`,
`npm run lint`, `npm test`, and Marketplace publication stay on a developer
machine. Packaging needs network access for
locked crates/npm dependencies, VS Code test hosts, and the exact upstream MCP
SDK license omitted from the crate archives. That license source is pinned to
the crate's own `.cargo_vcs_info.json` commit; an unavailable license fails
packaging.

For release builds, freeze the reviewed product commit, update Cargo/client and
release notes to the same version, then use the matching existing tag:
`npm run package:target -- --target win32-x64 --tag vVERSION`. Release mode
requires a clean tagged checkout and builds its own engine from it. Every
artifact retains `SOURCE.json`, checksums and license notices. The provenance
records the product license hash and its exact package paths: VSCE writes
`extension/LICENSE.txt`, while the standalone archive retains `LICENSE`. Later release
versions reuse this pipeline and repeat installed checks.

The shared Cargo/npm lock hashes identify the committed Git file bytes.
Separate `*_lock_checkout_sha256` fields retain the actual build checkout bytes,
which can use Windows CRLF line endings. The collector compares committed
hashes across targets and still verifies each exact artifact checksum.

Marketplace publication is local. On a clean checkout of the same tag, commit
a reviewed `release-gates/vVERSION.json` after the Release run. It names the
tagged artifact commit and version, and records `passed: true` plus an evidence
location for each of `publisher`, `native_vscode_mcp`, `extension_upgrade`,
`windows_linux_remote`, `runtime_requirements`, and `client_review`. Recording
the gates does not change the tested source. Missing gates prevent publication.

Download that run's `verified-packages` artifact. From `editors/vscode`, with
`GH_TOKEN` set and the artifact unpacked at `verified-packages`:

```sh
export RELEASE_TAG=vVERSION
export VERIFIED_RUN_ID=<release-run-id>
export GITHUB_REPOSITORY=naivej/dygnosis
export ARTIFACT_DIRECTORY=verified-packages
export RELEASE_GATES_FILE=../../release-gates/$RELEASE_TAG.json
node scripts/prepare-release.cjs
```

`prepare-release.cjs` checks the run, the tag commit, the changelog section,
and every artifact checksum. Publish the VSIX files from that artifact with `vsce`.

Publisher `dygnosis` is provisional. Confirm ownership in
[Marketplace publisher management](https://marketplace.visualstudio.com/manage)
before publication. Microsoft's [publishing guide](https://code.visualstudio.com/api/working-with-extensions/publishing-extension#secure-automated-publishing-to-visual-studio-marketplace)
recommends Entra authentication and describes the December 2026 global-PAT
retirement. Select and verify the owner's credential route before publication.
No credential is required by the compatibility workflow.
