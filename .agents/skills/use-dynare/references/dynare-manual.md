# Dynare manual

Read this when you need the meaning of a Dynare command, option, or block.
`dynare_list_options` lists the option names a command accepts. This manual states
what a name means.

## Which copy

The chapter files in `references/dynare-manual/` are the Dynare 7.2 reference
manual, the release Dygnosis checks. They were copied from tag `7.2`
(`d0295f151`), directory `doc/manual/source/`. Copyright and the license grant
are in `index.rst` (GNU Free Documentation License 1.3 or later, no Invariant
Sections). The skill `LICENSE` covers the skill. It does not cover these chapter
files.

Search this copy when `dynare_version` has not been run, and when it returns
`7.2`.

When `dynare_version` returns any other value, search the manual shipped with
that installation:

`<dynare-root>/doc/dynare-manual.html/_sources/<chapter>.rst.txt`

`<dynare-root>` is the install folder in `references/debugging.md`. `<chapter>`
is a file name from the table below. The install adds `.txt`. Name that file
and the version string in the report. If the `_sources` directory is absent,
say that the local manual source was not found.

## Chapters

| Need | File |
|---|---|
| A command, option, block, or function written in a `.mod` | `the-model-file.rst` |
| The `dynare` invocation, preprocessor messages, `M_` and `oo_` output | `running-dynare.rst` |
| Dates and `dseries` | `time-series.rst` |
| Reporting | `reporting.rst` |
| Parallel runs | `parallel-execution.rst` |
| Commands outside the model file | `dynare-misc-commands.rst` |
| Where an install keeps `matlab/` and `examples/` | `installation-and-configuration.rst` |
| Names of the examples shipped with Dynare | `examples.rst` |

## How to read a chapter

Search for the command heading, then read that block. A command heading starts
with `.. command::` or `.. command ::` and the command name. An option of that
command is a following `.. option::` line indented under it. The same option
name belongs to several commands. Read the option under the command you are
writing.

`the-model-file.rst` is 18,357 lines. Search it.

A `:ref:` label points at another heading. A `:cite:` key names a paper and
does not expand in this copy. `|version|` in `index.rst` is 7.2 for this bundle.
