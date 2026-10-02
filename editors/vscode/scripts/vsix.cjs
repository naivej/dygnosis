const assert = require("node:assert/strict");
const { Buffer } = require("node:buffer");
const crypto = require("node:crypto");
const path = require("node:path");
const yauzl = require("yauzl");
const { binaryName, productRoot, sha256, targetInfo } = require("./common.cjs");

function readZip(file) {
  return new Promise((resolve, reject) => {
    yauzl.open(file, { lazyEntries: true }, (error, zip) => {
      if (error) { reject(error); return; }
      const entries = new Map();
      let total = 0;
      zip.on("error", reject);
      zip.on("end", () => resolve(entries));
      zip.on("entry", entry => {
        try {
          assert.ok(!entry.fileName.includes("\\") && !entry.fileName.split("/").includes("..") && !entry.fileName.startsWith("/"), "Unsafe VSIX entry");
          assert.ok(!entries.has(entry.fileName), "Duplicate VSIX entry");
          const mode = (entry.externalFileAttributes >>> 16) & 0xffff;
          assert.notEqual(mode & 0xf000, 0xa000, "VSIX cannot contain symbolic links");
          if (entry.fileName.endsWith("/")) { zip.readEntry(); return; }
          total += entry.uncompressedSize;
          assert.ok(total <= 256 * 1024 * 1024, "Oversized VSIX");
          zip.openReadStream(entry, (streamError, stream) => {
            if (streamError) { zip.close(); reject(streamError); return; }
            const chunks = [];
            stream.on("data", chunk => chunks.push(chunk));
            stream.on("error", reject);
            stream.on("end", () => { entries.set(entry.fileName, { data: Buffer.concat(chunks), mode }); zip.readEntry(); });
          });
        } catch (entryError) { zip.close(); reject(entryError); }
      });
      zip.readEntry();
    });
  });
}
function assertBinaryTarget(bytes, target) {
  const { platform, arch } = targetInfo(target);
  if (platform === "win32") {
    assert.equal(bytes.subarray(0, 2).toString(), "MZ");
    const offset = bytes.readUInt32LE(0x3c);
    assert.equal(bytes.readUInt32LE(offset), 0x4550);
    assert.equal(bytes.readUInt16LE(offset + 4), arch === "arm64" ? 0xaa64 : 0x8664, "PE architecture differs from VSIX target");
  } else if (platform === "linux") {
    assert.equal(bytes.subarray(0, 4).toString("hex"), "7f454c46");
    assert.equal(bytes[4], 2); assert.equal(bytes[5], 1);
    assert.equal(bytes.readUInt16LE(18), arch === "arm64" ? 183 : 62, "ELF architecture differs from VSIX target");
  } else {
    assert.equal(bytes.readUInt32LE(0), 0xfeedfacf, "Expected a native 64-bit Mach-O binary");
    assert.equal(bytes.readUInt32LE(4), arch === "arm64" ? 0x0100000c : 0x01000007, "Mach-O architecture differs from VSIX target");
  }
}
async function inspectVsix(file, expected) {
  const entries = await readZip(file);
  const data = name => { assert.ok(entries.has(name), `Missing VSIX file: ${name}`); return entries.get(name).data; };
  const json = name => JSON.parse(data(`extension/${name}`).toString());
  const manifest = json("package.json");
  const source = json("SOURCE.json");
  assert.deepEqual(source, expected);
  assert.equal(manifest.name, "dygnosis"); assert.equal(manifest.displayName, "Dygnosis");
  assert.equal(manifest.publisher, source.publisher); assert.equal(manifest.version, source.version);
  assert.equal(manifest.icon, "media/logo_s.png");
  assert.deepEqual(manifest.extensionKind, ["workspace"]);
  const xml = data("extension.vsixmanifest").toString();
  assert.ok(xml.includes(`TargetPlatform="${source.target}"`), "VSIX has no matching target platform");
  assert.ok(xml.includes(`Version="${source.version}"`) && xml.includes(`Publisher="${source.publisher}"`));
  const hash = bytes => crypto.createHash("sha256").update(bytes).digest("hex");
  const binary = entries.get(`extension/bin/${binaryName(source.target)}`);
  assert.deepEqual([...entries.keys()].filter(name => name.startsWith("extension/bin/")), [`extension/bin/${binaryName(source.target)}`], "VSIX must contain exactly one platform binary");
  assert.ok(binary); assert.equal(hash(binary.data), source.binary_sha256);
  assertBinaryTarget(binary.data, source.target);
  if (targetInfo(source.target).platform !== "win32") assert.ok(binary.mode & 0o111, "VSIX lost the executable permission");
  assert.equal(hash(data("extension/media/logo_s.png")), source.logo_sha256);
  assert.equal(source.logo_sha256, await sha256(path.join(productRoot, "media/logo_s.png")));
  assert.equal(source.license.spdx, "GPL-3.0-or-later");
  assert.equal(source.license.source_file, "LICENSE");
  assert.equal(source.license.vsix_path, "extension/LICENSE.txt");
  assert.equal(source.license.standalone_path, "LICENSE");
  assert.equal(source.license.source_sha256, await sha256(path.join(productRoot, "LICENSE")));
  assert.equal(hash(data(source.license.vsix_path)), source.license.source_sha256);
  data("extension/out/extension.js"); data("extension/language-configuration.json"); data("extension/syntaxes/dynare.tmLanguage.json");
  for (const dependency of source.runtime_dependencies) data(`extension/${dependency}`);
  const notices = json("THIRD-PARTY-NOTICES.json");
  assert.ok(notices.some(notice => notice.kind === "toolchain"));
  assert.ok(notices.some(notice => notice.name === "vscode-languageclient"));
  for (const notice of notices) for (const license of notice.files) assert.equal(hash(data(`extension/${license.path}`)), license.sha256, `Changed license: ${license.path}`);
  for (const filename of entries.keys()) {
    assert.ok(!/^extension\/(?:src|tests|scripts|dist|\.test-data|\.vscode-test|\.npm-cache)\//.test(filename), `Development file in VSIX: ${filename}`);
    assert.ok(!filename.endsWith(".ts") && !filename.endsWith(".map"), `Source file in VSIX: ${filename}`);
  }
  return { files: entries.size, sha256: await sha256(file), executablePermission: targetInfo(source.target).platform === "win32" ? "not applicable" : true };
}
module.exports = { inspectVsix, assertBinaryTarget };
