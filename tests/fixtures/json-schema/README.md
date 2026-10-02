# JSON Schema metadata snapshots

These eight unmodified files were fetched from the official
[2020-12 schema](https://json-schema.org/draft/2020-12/schema) and its referenced
vocabulary metadata on 2026-10-03. `manifest.json` records each source URL and
SHA256. The local line-ending rule preserves the captured bytes.

They let the minimum VS Code validator reproduce its unsupported dynamic
reference warning without a network connection. The fixed Dygnosis schemas use
the host's built-in draft-07 metadata and make no schema requests.

The JSON Schema authors publish the repository under a choice of BSD-3-Clause
or AFL-3.0; see the [official license statement](https://github.com/json-schema-org/json-schema-spec#license).
These test fixtures use BSD-3-Clause. The full upstream notice is retained in
`LICENSE`, fetched from the [official license file](https://raw.githubusercontent.com/json-schema-org/json-schema-spec/main/LICENSE).
