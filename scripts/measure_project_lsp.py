"""Measure project-on/off LSP latency, work, and Windows process memory.

Run a release binary. Uses unsaved comment overlays, never edits archive files.
Example: python scripts/measure_project_lsp.py --binary ../target/release/dygnosis.exe
  --folder .agents/skills/dynare-copilot/references/examples --expected-roots 149
  --include .agents/skills/dynare-copilot/references/examples/set_parameters.m
  --samples 20 --output result.json
The first measured pass does not claim a cold filesystem cache.
"""
import argparse
import ctypes
import hashlib
import json
import math
import os
import pathlib
import platform
import queue
import re
import subprocess
import threading
import time


def uri(path):
    return pathlib.Path(path).resolve().as_uri()


def p95(samples):
    return sorted(samples)[math.ceil(0.95 * len(samples)) - 1]


def uri_key(value):
    return value.casefold() if os.name == "nt" else value


def diagnostic_semantics(items):
    # Input revisions in diagnostic data change on every comment edit. Written
    # locations, messages and related locations must stay equal.
    keys = ("range", "severity", "code", "message", "tags", "relatedInformation")
    return sorted(json.dumps({key: item[key] for key in keys if key in item}, sort_keys=True)
                  for item in items)


def verify_sample(publication, pulled, initial, version, hover, initial_hover):
    if publication.get("version") != version:
        raise RuntimeError("Timed diagnostic publication has an obsolete version")
    if publication["diagnostics"] != pulled:
        raise RuntimeError("Timed diagnostic publication differs from the current pull report")
    if diagnostic_semantics(pulled) != diagnostic_semantics(initial):
        raise RuntimeError("Comment edit changed or withdrew the active diagnostic report")
    if hover is None or hover.get("contents") != initial_hover.get("contents"):
        raise RuntimeError("Timed hover does not contain the current declaration information")


def verify_project(status, reports, expected):
    expected = {uri_key(value) for value in expected}
    roots = status["roots"]
    actual = [uri_key(entry["root_uri"]) for entry in roots]
    if len(actual) != len(set(actual)) or set(actual) != expected:
        raise RuntimeError("Project root set differs from CLI discovery")
    if (not status["complete"] or status["cancelled"] or status["discovery_failures"]
            or any(entry["state"] not in ("checked", "incomplete") or not entry["revision"]
                   for entry in roots)):
        raise RuntimeError("Project pass lacks terminal reports for all roots")
    counts = {state: sum(entry["state"] == state for entry in roots) for state in status["counts"]}
    if counts != status["counts"] or sum(counts.values()) != len(expected):
        raise RuntimeError("Project status counts differ from its root reports")
    published = [uri_key(item["uri"]) for item in reports]
    if len(published) != len(set(published)) or not expected.issubset(published):
        raise RuntimeError("Workspace diagnostic pull omitted or duplicated a root report")
    return {"root_count": len(roots), "counts": counts, "report_count": len(reports),
            "incomplete_roots": [entry["root_uri"] for entry in roots if entry["state"] == "incomplete"]}


def verify_include(before, after, include_uri):
    owners = [entry for entry in before["roots"]
              if uri_key(include_uri) in {uri_key(value) for value in entry["dependency_candidates"]}]
    if len(owners) < 2:
        raise RuntimeError("Shared include probe needs at least two proven dependency candidates")
    current = {uri_key(entry["root_uri"]): entry for entry in after["roots"]}
    if after["pass_revision"] <= before["pass_revision"] or after["metrics"]["completed_jobs"] < len(owners):
        raise RuntimeError("Shared include edit did not complete a new pass for all owners")
    for owner in owners:
        entry = current.get(uri_key(owner["root_uri"]))
        if entry is None or entry["revision"] == owner["revision"]:
            raise RuntimeError("Shared include edit retained an obsolete owner revision")
    return [entry["root_uri"] for entry in owners]


def verify_stopped(first, later, disabled):
    if disabled:
        if any(status["enabled"] or status["discovery"] != "disabled" or status["roots"]
               or status["metrics"]["completed_jobs"] != 0 for status in (first, later)):
            raise RuntimeError("Project off retained roots or continued background work")
    else:
        if any(not status["enabled"] or not status["cancelled"] or status["complete"]
               or status["counts"]["checking"] for status in (first, later)):
            raise RuntimeError("Cancellation did not pause project work")
        if (first["metrics"]["completed_jobs"] != later["metrics"]["completed_jobs"]
                or first["roots"] != later["roots"]
                or first["pass_revision"] != later["pass_revision"]):
            raise RuntimeError("Cancelled work committed a later result")


class WindowsProcess:
    def __init__(self, process):
        if os.name != "nt":
            raise RuntimeError("Reference memory/CPU measurement requires Windows")
        self.handle = process._handle
        self.kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        self.psapi = ctypes.WinDLL("psapi", use_last_error=True)

        class Counters(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_ulong), ("PageFaultCount", ctypes.c_ulong)] + [
                (name, ctypes.c_size_t) for name in (
                    "PeakWorkingSetSize", "WorkingSetSize", "QuotaPeakPagedPoolUsage",
                    "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                    "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]
        self.Counters = Counters
        self.psapi.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_ulong]
        self.kernel.GetProcessTimes.argtypes = [ctypes.c_void_p] + [ctypes.c_void_p] * 4

    def sample(self):
        counters = self.Counters()
        counters.cb = ctypes.sizeof(counters)
        if not self.psapi.GetProcessMemoryInfo(self.handle, ctypes.byref(counters), counters.cb):
            raise ctypes.WinError(ctypes.get_last_error())
        created, exited, kernel, user = (ctypes.c_ulonglong() for _ in range(4))
        if not self.kernel.GetProcessTimes(self.handle, *(ctypes.byref(v) for v in (created, exited, kernel, user))):
            raise ctypes.WinError(ctypes.get_last_error())
        return {"working_set_bytes": counters.WorkingSetSize, "peak_working_set_bytes": counters.PeakWorkingSetSize,
                "private_bytes": counters.PagefileUsage, "cpu_ms": (kernel.value + user.value) / 10000}


class Lsp:
    def __init__(self, binary):
        self.process = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                        stderr=subprocess.DEVNULL, bufsize=0,
                                        creationflags=subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0)
        self.monitoring = False
        try:
            self.messages = queue.Queue()
            self.next_id = 0
            self.statuses = []
            self.stats = WindowsProcess(self.process)
            self.peak = 0
            self.monitoring = True
            threading.Thread(target=self.read, daemon=True).start()
            threading.Thread(target=self.monitor, daemon=True).start()
        except BaseException:
            self.close()
            raise

    def monitor(self):
        while self.monitoring:
            try:
                self.peak = max(self.peak, self.stats.sample()["working_set_bytes"])
            except OSError:
                return
            time.sleep(0.01)

    def read(self):
        stream = self.process.stdout
        while True:
            length = None
            while True:
                line = stream.readline()
                if not line:
                    return
                if line == b"\r\n":
                    break
                if line.startswith(b"Content-Length:"):
                    length = int(line.split(b":", 1)[1])
            body = bytearray()
            while len(body) < length:
                chunk = stream.read(length - len(body))
                if not chunk:
                    return
                body.extend(chunk)
            self.messages.put(json.loads(body))

    def send(self, method, params, request=False):
        message = {"jsonrpc": "2.0", "method": method, "params": params}
        if request:
            self.next_id += 1
            message["id"] = self.next_id
        self.write(message)
        return message.get("id")

    def write(self, message):
        body = json.dumps(message, separators=(",", ":")).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def next(self, timeout=60):
        message = self.messages.get(timeout=timeout)
        if message.get("method") and "id" in message:
            self.write({"jsonrpc": "2.0", "id": message["id"], "result": None})
        if message.get("method") == "dynare/projectStatusChanged":
            self.statuses.append(message["params"])
        return message

    def response(self, request_id):
        deadline = time.perf_counter() + 60
        while True:
            remaining = deadline - time.perf_counter()
            if remaining <= 0:
                raise TimeoutError(f"LSP request {request_id} did not finish within 60 s")
            message = self.next(remaining)
            if message.get("id") == request_id and "method" not in message:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]

    def command(self, command):
        return self.response(self.send("workspace/executeCommand", {"command": command, "arguments": []}, True))

    def pull(self, document_uri):
        return self.response(self.send("textDocument/diagnostic", {"textDocument": {"uri": document_uri}}, True))["items"]

    def reports(self):
        return self.response(self.send("workspace/diagnostic", {"previousResultIds": []}, True))["items"]

    def wait_complete(self, pass_revision=None):
        deadline = time.perf_counter() + 120
        while time.perf_counter() < deadline:
            status = self.command("dynare/projectStatus")
            if status["complete"] and (pass_revision is None or status["pass_revision"] >= pass_revision):
                return status
            time.sleep(0.02)
        raise TimeoutError("Project did not complete within 120 s")

    def close(self):
        self.monitoring = False
        try:
            if self.process.poll() is None:
                self.process.terminate()
            try:
                self.process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
        finally:
            try:
                self.process.stdin.close()
            finally:
                self.process.stdout.close()


def discover(folder):
    roots = []
    for directory, subdirectories, files in os.walk(folder):
        subdirectories[:] = [name for name in subdirectories if not name.startswith("+")]
        roots.extend(pathlib.Path(directory) / name for name in files if pathlib.Path(name).suffix == ".mod")
    return sorted(roots)


def run(binary, folder, active, include, samples, enabled, expected):
    client = Lsp(binary)
    try:
        started = time.perf_counter()
        init = client.send("initialize", {"workspaceFolders": [{"uri": uri(folder), "name": folder.name}],
            "capabilities": {"experimental": {"dygnosis": {"projectStatusChanged": True}}},
            "initializationOptions": {"dynare": {"configuration": {"schemaVersion": 1,
                "loose": {"projectDiagnostics": enabled}, "folders": []}}}}, True)
        server = client.response(init)["serverInfo"]
        client.send("dynare/activeModelChanged", {"root_uri": uri(active)})
        client.send("initialized", {})
        source = active.read_text(encoding="utf-8", errors="replace")
        variable = re.search(r"(?m)^\s*var\s+([A-Za-z_][A-Za-z_0-9]*)", source)
        hover_offset = variable.start(1) if variable else 0
        hover_position = {"line": source[:hover_offset].count("\n"), "character": len(source[:hover_offset].rsplit("\n", 1)[-1].encode("utf-16-le")) // 2}
        client.send("textDocument/didOpen", {"textDocument": {"uri": uri(active), "languageId": "dynare", "version": 1, "text": source}})
        initial = client.wait_complete() if enabled else client.command("dynare/projectStatus")
        first_ms = (time.perf_counter() - started) * 1000
        first_memory = client.stats.sample()
        coverage = []
        if enabled:
            coverage.append(verify_project(initial, client.reports(), expected))
        initial_diagnostics = client.pull(uri(active))
        initial_hover = client.response(client.send("textDocument/hover", {"textDocument": {"uri": uri(active)}, "position": hover_position}, True))
        if initial_hover is None:
            raise RuntimeError("Active model has no declaration hover at the selected position")
        repeated = []
        if enabled:
            for _ in range(2):
                start = time.perf_counter()
                status = client.command("dynare/recheckProject")
                done = client.wait_complete(status["pass_revision"])
                repeated.append({"completion_ms": (time.perf_counter() - start) * 1000, "metrics": done["metrics"]})
                coverage.append(verify_project(done, client.reports(), expected))
        # On samples overlap a fresh archive pass. Both processes keep exactly
        # the same file open and receive identical comment-only unsaved edits.
        if enabled:
            client.command("dynare/recheckProject")
        edits, hovers, proofs = [], [], []
        for sample in range(samples):
            version = sample + 2
            start = time.perf_counter()
            client.send("textDocument/didChange", {"textDocument": {"uri": uri(active), "version": version},
                "contentChanges": [{"text": source + f"\n// project latency sample {sample}\n"}]})
            hover_id = client.send("textDocument/hover", {"textDocument": {"uri": uri(active)}, "position": hover_position}, True)
            diagnostic_ms, hover_ms, publication, hover = None, None, None, None
            deadline = start + 60
            while diagnostic_ms is None or hover_ms is None:
                remaining = deadline - time.perf_counter()
                if remaining <= 0:
                    raise TimeoutError(f"Edit/hover sample {sample} did not finish within 60 s")
                message = client.next(remaining)
                elapsed = (time.perf_counter() - start) * 1000
                if message.get("method") == "textDocument/publishDiagnostics" and message["params"]["uri"] == uri(active) and message["params"].get("version") == version:
                    if diagnostic_ms is None:
                        diagnostic_ms, publication = elapsed, message["params"]
                if message.get("id") == hover_id and "method" not in message:
                    if "error" in message:
                        raise RuntimeError(message["error"])
                    hover_ms, hover = elapsed, message["result"]
            pulled = client.pull(uri(active))
            verify_sample(publication, pulled, initial_diagnostics, version, hover, initial_hover)
            proofs.append({"version": version, "diagnostic_count": len(pulled), "current_pull_equal": True,
                           "comment_diagnostics_equal": True, "declaration_hover_equal": True})
            edits.append(diagnostic_ms)
            hovers.append(hover_ms)
            time.sleep(0.05)
        settled = client.wait_complete() if enabled else client.command("dynare/projectStatus")
        if enabled:
            coverage.append(verify_project(settled, client.reports(), expected))
        retained = client.stats.sample()
        include_result = None
        if include is not None:
            start = time.perf_counter()
            initially_exists = include.exists()
            affected = sum(uri_key(uri(include)) in [uri_key(candidate) for candidate in entry["dependency_candidates"]]
                           for entry in settled["roots"])
            client.send("textDocument/didOpen", {"textDocument": {"uri": uri(include), "languageId": "dynare", "version": 1,
                "text": (include.read_text(encoding="utf-8", errors="replace") if initially_exists else "") + "\n// shared include measurement\n"}})
            # Barrier guarantees the input event has been handled before polling.
            client.response(client.send("textDocument/diagnostic", {"textDocument": {"uri": uri(include)}}, True))
            done = client.wait_complete() if enabled else client.command("dynare/projectStatus")
            include_result = {"completion_ms": (time.perf_counter() - start) * 1000, "initially_exists": initially_exists, "affected_roots": affected, "status": done}
            if enabled:
                include_result["rechecked_owners"] = verify_include(settled, done, uri(include))
                coverage.append(verify_project(done, client.reports(), expected))
            client.send("textDocument/didClose", {"textDocument": {"uri": uri(include)}})
            client.pull(uri(active))
            if enabled:
                restored = client.wait_complete()
                coverage.append(verify_project(restored, client.reports(), expected))
                include_result["close_status"] = restored
        cancel = None
        queued_before_off = None
        if enabled:
            queued = client.command("dynare/recheckProject")
            if queued["complete"] or not (queued["counts"]["pending"] + queued["counts"]["checking"]
                                            or queued["discovery"] in ("pending", "discovering")):
                raise RuntimeError("Cancellation probe did not start with queued work")
            start = time.perf_counter()
            result = client.command("dynare/cancelProject")
            cancel = {"response_ms": (time.perf_counter() - start) * 1000, "queued_status": queued, "status": result}
            time.sleep(0.5)
            stopped = client.command("dynare/projectStatus")
            verify_stopped(result, stopped, False)
            cancel["settled_status"] = stopped
            queued_before_off = client.command("dynare/recheckProject")
            if queued_before_off["complete"]:
                raise RuntimeError("Disable probe did not start with queued work")
        client.send("workspace/didChangeConfiguration", {"settings": {"dynare": {"projectDiagnostics": False}}})
        off_status = client.command("dynare/projectStatus")
        time.sleep(0.5)
        after_off_status = client.command("dynare/projectStatus")
        verify_stopped(off_status, after_off_status, True)
        off_reports = client.reports()
        root_reports = {uri_key(item["uri"]) for item in off_reports} & {uri_key(value) for value in expected}
        if root_reports != {uri_key(uri(active))}:
            raise RuntimeError("Project off retained a diagnostic contribution from an unopened root")
        after_off = client.stats.sample()
        return {"enabled": enabled, "server": server, "first_completion_ms": first_ms, "first_status": initial,
            "first_memory": first_memory, "repeated": repeated, "edit_ms": edits, "hover_ms": hovers,
            "edit_p95_ms": p95(edits), "hover_p95_ms": p95(hovers), "settled_status": settled,
            "retained_memory": retained, "sampled_peak_working_set_bytes": client.peak,
            "include_edit": include_result, "cancellation": cancel, "memory_after_off": after_off,
            "queued_before_off": queued_before_off, "off_status": after_off_status,
            "sample_proofs": proofs, "coverage_checks": coverage,
            "functional_checks_passed": True}
    finally:
        client.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--folder", required=True, type=pathlib.Path)
    parser.add_argument("--active", type=pathlib.Path)
    parser.add_argument("--include", type=pathlib.Path, required=True)
    parser.add_argument("--expected-roots", type=int, required=True)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    report = {"schema_version": 2, "binary": str(args.binary),
              "passed": False, "status": "validation_incomplete"}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    try:
        measure(args, report)
    except BaseException as error:
        report["passed"] = False
        report["status"] = "failed"
        report["failure"] = f"{type(error).__name__}: {error}"
        raise
    finally:
        args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")


def measure(args, report):
    if args.samples < 20:
        raise ValueError("Release measurements need at least 20 comparable samples")
    roots = discover(args.folder)
    if not roots:
        raise ValueError("Folder contains no CLI-discoverable .mod roots")
    if len(roots) != args.expected_roots:
        raise ValueError(f"Expected {args.expected_roots} roots, discovered {len(roots)}")
    active = args.active or max(roots, key=lambda path: path.stat().st_size)
    if active.resolve() not in [root.resolve() for root in roots]:
        raise ValueError("Active model must belong to the discovered root set")
    expected = [uri(root) for root in roots]
    report.update({"hardware": {"platform": platform.platform(), "processor": platform.processor(), "logical_cpus": os.cpu_count()},
        "binary": str(args.binary.resolve()), "binary_bytes": args.binary.stat().st_size,
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "method": {"samples": args.samples, "active": str(active.resolve()), "folder": str(args.folder.resolve()), "roots": len(roots),
            "root_uris": expected, "include": str(args.include.resolve()), "stop_settle_ms": 500,
            "first_pass_cache": "not assumed cold", "memory_poll_ms": 10, "peak_gate": "OS PeakWorkingSetSize",
            "edit_pause_ms": 50, "latency": "stdio roundtrip; comment overlay; on samples overlap archive Recheck; current pull/hover checked after timing"},
        "status": "measurement_incomplete"})
    off = run(args.binary.resolve(), args.folder.resolve(), active.resolve(), args.include, args.samples, False, expected)
    report["off"] = off
    on = run(args.binary.resolve(), args.folder.resolve(), active.resolve(), args.include, args.samples, True, expected)
    report["on"] = on
    added_edit = on["edit_p95_ms"] - off["edit_p95_ms"]
    added_hover = on["hover_p95_ms"] - off["hover_p95_ms"]
    added_retained = on["retained_memory"]["working_set_bytes"] - off["retained_memory"]["working_set_bytes"]
    added_peak = on["memory_after_off"]["peak_working_set_bytes"] - off["memory_after_off"]["peak_working_set_bytes"]
    gates = {"diagnostic_delay": added_edit <= max(50, 0.2 * off["edit_p95_ms"]), "hover_delay": added_hover <= 50,
        "first_completion": on["first_completion_ms"] <= 25000,
        "repeated_completion": all(item["completion_ms"] <= 15000 for item in on["repeated"]),
        "retained_memory": added_retained <= 128 * 1024**2, "peak_memory": added_peak <= 256 * 1024**2,
        "protocol_and_coverage": off["functional_checks_passed"] and on["functional_checks_passed"]}
    report.update({"added": {"edit_p95_ms": added_edit, "hover_p95_ms": added_hover,
            "retained_working_set_bytes": added_retained, "peak_working_set_bytes": added_peak},
            "gates": gates, "passed": all(gates.values()), "status": "completed"})
    print(json.dumps({"output": str(args.output), "roots": len(roots), "added": report["added"], "gates": gates, "passed": report["passed"]}, indent=2))
    if not report["passed"]:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
