"""Measure project-on/off LSP latency, work, and Windows process memory.

Run a release binary. Uses unsaved comment overlays, never edits archive files.
Example: python scripts/measure_project_lsp.py --binary ../target/release/dygnosis.exe
  --folder .agents/skills/dynare-copilot/references/examples --samples 20 --output result.json
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
        self.messages = queue.Queue()
        self.pending = []
        self.next_id = 0
        self.statuses = []
        self.stats = WindowsProcess(self.process)
        self.peak = 0
        self.monitoring = True
        threading.Thread(target=self.read, daemon=True).start()
        threading.Thread(target=self.monitor, daemon=True).start()

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
        while True:
            message = self.next()
            if message.get("id") == request_id and "method" not in message:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message["result"]
            self.pending.append(message)

    def command(self, command):
        return self.response(self.send("workspace/executeCommand", {"command": command, "arguments": []}, True))

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
        self.process.terminate()
        self.process.wait(timeout=10)


def discover(folder):
    roots = []
    for directory, subdirectories, files in os.walk(folder):
        subdirectories[:] = [name for name in subdirectories if not name.startswith("+")]
        roots.extend(pathlib.Path(directory) / name for name in files if pathlib.Path(name).suffix == ".mod")
    return sorted(roots)


def run(binary, folder, active, include, samples, enabled):
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
        repeated = []
        if enabled:
            for _ in range(2):
                start = time.perf_counter()
                status = client.command("dynare/recheckProject")
                done = client.wait_complete(status["pass_revision"])
                repeated.append({"completion_ms": (time.perf_counter() - start) * 1000, "metrics": done["metrics"]})
        # On samples overlap a fresh archive pass. Both processes keep exactly
        # the same file open and receive identical comment-only unsaved edits.
        if enabled:
            client.command("dynare/recheckProject")
        edits, hovers = [], []
        for sample in range(samples):
            version = sample + 2
            start = time.perf_counter()
            client.send("textDocument/didChange", {"textDocument": {"uri": uri(active), "version": version},
                "contentChanges": [{"text": source + f"\n// project latency sample {sample}\n"}]})
            hover_id = client.send("textDocument/hover", {"textDocument": {"uri": uri(active)}, "position": hover_position}, True)
            diagnostic_ms, hover_ms = None, None
            while diagnostic_ms is None or hover_ms is None:
                message = client.next()
                elapsed = (time.perf_counter() - start) * 1000
                if message.get("method") == "textDocument/publishDiagnostics" and message["params"]["uri"] == uri(active) and message["params"].get("version") == version:
                    diagnostic_ms = elapsed
                if message.get("id") == hover_id and "method" not in message:
                    if "error" in message:
                        raise RuntimeError(message["error"])
                    hover_ms = elapsed
            edits.append(diagnostic_ms)
            hovers.append(hover_ms)
            time.sleep(0.05)
        settled = client.wait_complete() if enabled else client.command("dynare/projectStatus")
        retained = client.stats.sample()
        include_result = None
        if include is not None:
            start = time.perf_counter()
            initially_exists = include.exists()
            affected = sum(uri(include).casefold() in [candidate.casefold() for candidate in entry["dependency_candidates"]]
                           for entry in settled["roots"])
            client.send("textDocument/didOpen", {"textDocument": {"uri": uri(include), "languageId": "dynare", "version": 1,
                "text": (include.read_text(encoding="utf-8", errors="replace") if initially_exists else "") + "\n// shared include measurement\n"}})
            # Barrier guarantees the input event has been handled before polling.
            client.response(client.send("textDocument/diagnostic", {"textDocument": {"uri": uri(include)}}, True))
            done = client.wait_complete() if enabled else client.command("dynare/projectStatus")
            include_result = {"completion_ms": (time.perf_counter() - start) * 1000, "initially_exists": initially_exists, "affected_roots": affected, "status": done}
            client.send("textDocument/didClose", {"textDocument": {"uri": uri(include)}})
        cancel = None
        if enabled:
            client.command("dynare/recheckProject")
            start = time.perf_counter()
            result = client.command("dynare/cancelProject")
            cancel = {"response_ms": (time.perf_counter() - start) * 1000, "status": result}
        client.send("workspace/didChangeConfiguration", {"settings": {"dynare": {"projectDiagnostics": False}}})
        client.command("dynare/projectStatus")
        time.sleep(0.5)
        after_off = client.stats.sample()
        return {"enabled": enabled, "server": server, "first_completion_ms": first_ms, "first_status": initial,
            "first_memory": first_memory, "repeated": repeated, "edit_ms": edits, "hover_ms": hovers,
            "edit_p95_ms": p95(edits), "hover_p95_ms": p95(hovers), "settled_status": settled,
            "retained_memory": retained, "sampled_peak_working_set_bytes": client.peak,
            "include_edit": include_result, "cancellation": cancel, "memory_after_off": after_off}
    finally:
        client.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", required=True, type=pathlib.Path)
    parser.add_argument("--folder", required=True, type=pathlib.Path)
    parser.add_argument("--active", type=pathlib.Path)
    parser.add_argument("--include", type=pathlib.Path)
    parser.add_argument("--samples", type=int, default=20)
    parser.add_argument("--output", required=True, type=pathlib.Path)
    args = parser.parse_args()
    if args.samples < 20:
        parser.error("Release measurements need at least 20 comparable samples")
    roots = discover(args.folder)
    if not roots:
        parser.error("Folder contains no CLI-discoverable .mod roots")
    active = args.active or max(roots, key=lambda path: path.stat().st_size)
    off = run(args.binary.resolve(), args.folder.resolve(), active.resolve(), args.include, args.samples, False)
    on = run(args.binary.resolve(), args.folder.resolve(), active.resolve(), args.include, args.samples, True)
    added_edit = on["edit_p95_ms"] - off["edit_p95_ms"]
    added_hover = on["hover_p95_ms"] - off["hover_p95_ms"]
    added_retained = on["retained_memory"]["working_set_bytes"] - off["retained_memory"]["working_set_bytes"]
    added_peak = on["sampled_peak_working_set_bytes"] - off["sampled_peak_working_set_bytes"]
    gates = {"diagnostic_delay": added_edit <= max(50, 0.2 * off["edit_p95_ms"]), "hover_delay": added_hover <= 50,
        "first_completion": on["first_completion_ms"] <= 25000,
        "repeated_completion": all(item["completion_ms"] <= 15000 for item in on["repeated"]),
        "retained_memory": added_retained <= 128 * 1024**2, "peak_memory": added_peak <= 256 * 1024**2}
    report = {"schema_version": 1, "hardware": {"platform": platform.platform(), "processor": platform.processor(), "logical_cpus": os.cpu_count()},
        "binary": str(args.binary.resolve()), "binary_bytes": args.binary.stat().st_size,
        "binary_sha256": hashlib.sha256(args.binary.read_bytes()).hexdigest(),
        "method": {"samples": args.samples, "active": str(active.resolve()), "folder": str(args.folder.resolve()), "roots": len(roots),
            "first_pass_cache": "not assumed cold", "memory_poll_ms": 10, "edit_pause_ms": 50, "latency": "stdio roundtrip; comment overlay; on samples overlap archive Recheck"},
        "off": off, "on": on, "added": {"edit_p95_ms": added_edit, "hover_p95_ms": added_hover,
            "retained_working_set_bytes": added_retained, "peak_working_set_bytes": added_peak}, "gates": gates, "passed": all(gates.values())}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2), encoding="utf-8")
    print(json.dumps({"output": str(args.output), "roots": len(roots), "added": report["added"], "gates": gates, "passed": report["passed"]}, indent=2))


if __name__ == "__main__":
    main()
