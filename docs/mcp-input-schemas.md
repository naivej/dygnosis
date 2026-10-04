# MCP input schemas

Dygnosis advertises its parameterized tools with an explicit JSON Schema
draft-07 declaration. Tool names, arguments, required fields, nullable values,
defaults, and results retain their existing meanings. The tool with no
arguments retains its empty object schema.

The minimum supported editor, VS Code 1.102.0, validates draft-07 schemas
without downloading schema metadata. Its native MCP discovery rejects the
default 2020-12 declaration. An explicit alternative dialect is allowed by the
[MCP schema dialect contract](https://modelcontextprotocol.io/specification/2025-11-25/basic#schema-dialect).
