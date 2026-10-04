# Trace an effective model

Show effective model opens include and macro expansion in a read-only editor. The preview shows the model before Dynare transforms equations. Edit the written files, then use Refresh effective model to update the preview.

Place the cursor in a mapped model row, or select part of one row. Go to written source opens its verified written portion beside the preview. A row assembled from several files offers a file picker. The picker identifies the model scope and expansion occurrence; repeated macro copies keep their own identity. A selection crossing several rows has no unambiguous jump. Text outside mapped rows has no source action.

Show macro origins offers the row's verified macro directive and body sites. Labels identify the directive kind, file, frame order, and loop variable/value when available. Choosing an item opens that written site. These locations come from the engine's trace; the extension does not infer them from equation numbers or expanded text.

The three commands appear in the Command Palette while a preview is active. Preview title and context menus follow `dynare.editorActions`: `toolbar`, `contextMenu`, or neither. Default keyboard shortcuts apply only in a preview editor:

| Action | Windows/Linux | macOS |
|---|---|---|
| Go to written source | Ctrl+Alt+G | Cmd+Alt+G |
| Show macro origins | Ctrl+Alt+M | Cmd+Alt+M |
| Refresh effective model | Ctrl+Alt+R | Cmd+Alt+R |

VS Code's Keyboard Shortcuts editor can change these bindings. Pickers use VS Code's native keyboard controls and theme.

Each preview retains its chosen root and input revision. It can refresh after that root closes, and several previews can refer to different roots. Source edits, dependency changes, settings changes, and a language-server restart disable origin actions until Refresh. The readable preview remains available. Refresh replaces text and navigation together; an older response cannot overwrite a newer request. Refresh does not jump to a location based on the previous cursor mapping.

The extension checks the snapshot after a picker and after loading a source file. A missing source, changed document version, changed input revision, or unavailable mapping cancels the jump. An unopened source may load with a different native URI spelling, including drive-letter case on Windows; the same native file still matches. A disk file opened with unchanged contents retains its input revision. An unsaved source that the engine has not observed supplies no safe jump.

Incomplete expansion is labelled and has no origin actions. An older engine can keep the basic text preview and Refresh while navigation remains unavailable. Invalid advertised navigation data reports an engine-update recovery action. Effective previews use the `dygnosis-effective:` scheme and stay outside the language server's analysis selector.
