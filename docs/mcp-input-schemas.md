# MCP input schemas

Dygnosis advertises its parameterized tools with an explicit JSON Schema
draft-07 declaration. The schemas come from the same Rust parameter types that
deserialize tool calls, using Schemars' draft-07 generator. Tool names,
arguments, required fields, nullable values, defaults, and results retain their
existing meanings.

The minimum supported editor, VS Code 1.102.0, includes draft-07 metadata in its
JSON validator. Its native MCP discovery rejects the SDK's default 2020-12
declaration because that external meta-schema uses dynamic references the
validator does not support. The declared draft-07 schemas can be checked without
downloading schema metadata. The tool with no arguments retains its empty
object schema.

An explicit alternative dialect is allowed by the
[MCP schema dialect contract](https://modelcontextprotocol.io/specification/2025-11-25/basic#schema-dialect).

For the minimum-host validator regression, build the binary and run:

```powershell
node tests/native_mcp_schema.cjs <dygnosis.exe> <VS Code 1.102.0 resources/app>
```

The check reads the real stdio `tools/list` response and validates every input
schema using that host's bundled JSON language service. It also reproduces the
old declaration's unsupported-meta warning using pinned official metadata
snapshots, and confirms that the fixed schemas make no metadata requests.
It complements actual
native discovery and invocation checks; direct stdio discovery alone cannot
detect tools that VS Code omits during validation.
