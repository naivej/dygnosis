# Project diagnostics

Project diagnostics check unopened saved `.mod` models beneath file-backed workspace folders. Discovery uses the same recursive walk as `dygnosis check`, including its generated `+` directory skip. Open `.dyn`, excluded models, loose files, and untitled documents retain ordinary editor diagnostics.

## Configuration

`dynare.projectDiagnostics` is a window boolean, default `true`. Send it in `dynare.configuration.loose.projectDiagnostics` in the existing schema-version 1 complete configuration snapshot. The legacy `dynare.projectDiagnostics` route is also accepted. Folder entries do not override this window switch.

`dynare.projectExcludePaths` is a resource list of strings, default `[]`. Send the resolved list in each folder entry's `settings.projectExcludePaths`; loose settings provide the fallback. The most specific containing workspace folder supplies exclusions and search paths. Patterns are relative to that folder; `*` and `?` stay within one path component, `**` crosses directories, `**/` also matches no directory, and a trailing slash includes the directory's descendants. Both slash styles are accepted; Windows matching ignores case. Invalid list values fall back to no exclusions; invalid entries are ignored and explained in the LSP log.

Exclusions select background roots only. An excluded include remains a dependency of its owners. Opening an excluded root still checks it. Turning project diagnostics off cancels work and clears project contributions while retaining reports belonging to open compilation units.

## Capability and protocol

Check `initialize`'s `capabilities.experimental.dygnosis.projectDiagnostics` before using this feature:

```json
{
  "schema_version": 1,
  "status_command": "dynare/projectStatus",
  "recheck_command": "dynare/recheckProject",
  "cancel_command": "dynare/cancelProject",
  "active_model_notification": "dynare/activeModelChanged",
  "status_notification": "dynare/projectStatusChanged",
  "typing_pause_ms": 250
}
```

Opt in to status notifications with client `capabilities.experimental.dygnosis.projectStatusChanged = true`. Notifications and command results have the same schema. Reject unsupported schemas rather than inferring a layout. On reconnect, execute `dynare/projectStatus` with no arguments.

Send `dynare/activeModelChanged` with `{"root_uri":"file:///.../chosen.mod"}` or `{"root_uri":null}`. The URI identifies the explicitly chosen model owner; clients must not silently choose an include's owner. That choice stays authoritative until updated or cleared. Opening, editing, saving, hovering, or querying another model updates only the fallback. Null restores recent-input priority; a server restart resets the explicit choice. Project checking starts after `initialized`, including a folder-only session.

Execute `dynare/recheckProject` to rerun discovery and all selected roots. Execute `dynare/cancelProject` to stop this pass; completed results remain, pending models remain pending, and cancelled jobs cannot publish. Cancellation lasts until a subsequent file edit/change or Recheck. It does not change the persistent setting. Recheck while off leaves the feature disabled.

The schema-version 1 status contains:

| Field | Meaning |
| --- | --- |
| `pass_revision` | Increasing identifier for the current pass in this server instance. |
| `enabled` | Persistent project switch. |
| `discovery` | `pending`, `discovering`, `complete`, `cancelled`, or `disabled`. |
| `cancelled` | The current pass has been cancelled. |
| `complete` | Discovery and queued checking have finished; failed/incomplete roots may remain. |
| `coverage_complete` | Complete with no discovery failure, incomplete root, or failed root. This does not mean no diagnostic Errors. |
| `counts` | Counts keyed by `pending`, `checking`, `checked`, `incomplete`, `failed`, and `excluded`. |
| `roots` | Root entries described below, sorted by URI. |
| `discovery_failures` | Folder URI and infrastructure failure text for unsuccessful walks. |
| `metrics` | `discovery_ms`, `analysis_ms`, `completed_jobs`, `reused_jobs`, and `elapsed_ms` for performance measurement. `analysis_ms` covers completed jobs, including compact input validation on reused reports. Wall timings are not solver work. |

Each root has `root_uri`, `state`, `revision` (nullable), `errors`, `warnings`, `failure` (nullable), and `dependency_candidates` (sorted file URIs). Watch every dependency candidate regardless of extension; this includes missing include search candidates, companion candidates, includepath directories, and loader files. Send ordinary `workspace/didChangeWatchedFiles` events for their creation, change, or deletion. Also watch root `.mod` creation/rename/deletion beneath workspace folders. The client needs no include parser or search resolver.

`checked` includes models that have diagnostic Errors. `incomplete` describes incomplete include/macro expansion; `failed` is an infrastructure/read failure. Neither pending nor incomplete nor failed roots may be presented as clean. Excluded entries have no background contribution. Counts and project coverage are separate from active-model counts.

## Reports and input lifetime

The server publishes one contribution per root whether it is open or discovered. Push diagnostics and workspace pull read the same merged report store. Changed inputs withdraw affected contributions immediately, so old ranges cannot acquire a newer document version. Open owners rebuild normally; unaffected owners retain their contributions. Pending roots have no current report or authoritative result counts. A prior report may remain privately cached and is reusable only after its exact input stamps are validated on the worker. Related locations and fixes retain written-file and root context. Closing a root or include removes its overlay and schedules its saved owners. Native path aliases replace the same overlay/version; Unix and virtual URI case remains distinct.

Changed inputs coalesce after the advertised `typing_pause_ms`. Unchanged checked reports can be reused; Recheck always computes again. The `metrics` field reports work and timing only; it does not claim a performance limit.
