const { execFileSync } = require("node:child_process");
const path = require("node:path");
if (process.platform !== "win32") throw new Error("Use a hidden/windowless platform test runner; this helper currently launches Windows hosts.");
execFileSync("powershell.exe", ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", path.join(__dirname, "run-host.ps1")], { stdio: "inherit", windowsHide: true });
