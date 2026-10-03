// Documentation illustrations, not screenshots. Run with an optional absolute
// @resvg/resvg-js module path. SVG sources and rendered PNGs stay together.
const fs = require("node:fs");
const path = require("node:path");
const { Resvg } = require(process.argv[2] || "@resvg/resvg-js");
const output = path.join(__dirname, "../media/readme");
fs.mkdirSync(path.join(output, "source"), { recursive: true });
const escape = value => String(value).replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll('"', "&quot;");
const rect = (x, y, width, height, fill, stroke = "none", radius = 0) =>
  `<rect x="${x}" y="${y}" width="${width}" height="${height}" rx="${radius}" fill="${fill}" stroke="${stroke}"/>`;
const text = (x, y, value, size = 21, fill = "#223047", weight = 400, code = false) =>
  `<text x="${x}" y="${y}" font-size="${size}" fill="${fill}" font-weight="${weight}"${code ? ' font-family="Consolas, monospace"' : ""}>${escape(value)}</text>`;
const line = (x1, y1, x2, y2, color = "#d4dae3", width = 1) =>
  `<path d="M${x1} ${y1}L${x2} ${y2}" stroke="${color}" stroke-width="${width}"/>`;
const check = (x, y, color = "#16724e") => `<path d="M${x} ${y}l5 5 10-12" stroke="${color}" stroke-width="2.5"/>`;
const chevron = (x, y) => `<path d="M${x} ${y}l5 5 5-5" stroke="#52657d" stroke-width="1.8"/>`;
const button = (x, y, width, value, accent = false) => rect(x, y, width, 37, accent ? "#0861b5" : "#f5f7fa", accent ? "#0861b5" : "#bbc5d2", 4)
  + text(x + 12, y + 25, value.replace("  ⌄", ""), 19, accent ? "white" : "#223047")
  + (value.includes("⌄") ? chevron(x + width - 21, y + 15) : "");
const chrome = (tab, second = "") => rect(16, 16, 1168, 664, "white", "#bbc5d2", 10)
  + rect(16, 16, 1168, 42, "#223047", "none", 10)
  + rect(16, 46, 1168, 12, "#223047")
  + text(38, 44, "Visual Studio Code", 20, "#f7f9fd", 600)
  + text(995, 43, "−    □    ×", 21, "#f7f9fd")
  + rect(16, 58, 1168, 42, "#eef1f6") + text(38, 86, tab, 20, "#223047", 600)
  + (second ? text(650, 86, second, 20, "#223047", 600) : "")
  + line(16, 100, 1184, 100)
  + rect(16, 650, 1168, 30, "#0861b5")
  + text(34, 672, "Dygnosis", 17, "white")
  + text(1077, 672, "Dynare", 17, "white")
  + text(1058, 705, "Illustration", 16, "#627084");
function save(name, title, description, body) {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 716" role="img" aria-labelledby="title description" fill="none" stroke-linecap="round" stroke-linejoin="round"><title id="title">${escape(title)}</title><desc id="description">${escape(description)}</desc><g font-family="Arial, sans-serif">${rect(0, 0, 1200, 716, "#ffffff")}${body}</g></svg>`;
  fs.writeFileSync(path.join(output, "source", `${name}.svg`), svg);
  const raster = new Resvg(svg, { fitTo: { mode: "width", value: 1800 }, font: { loadSystemFonts: true, defaultFontFamily: "Arial" } });
  fs.writeFileSync(path.join(output, `${name}.png`), raster.render().asPng());
}

let workbench = chrome("model.mod");
workbench += rect(16, 100, 275, 550, "#f3f5f8") + line(291, 100, 291, 650);
workbench += text(34, 128, "EXPLORER", 17, "#516079", 600)
  + chevron(36, 151) + text(55, 163, "models", 21, "#223047", 600)
  + rect(17, 177, 273, 35, "#dceafb") + text(54, 202, "model.mod", 21)
  + text(54, 239, "stoch.mod", 21) + text(54, 276, "policy.mod", 21)
  + line(17, 294, 290, 294)
  + text(34, 324, "DYNARE MODEL", 17, "#516079", 600)
  + chevron(36, 348) + text(55, 359, "Aggregate counts", 21)
  + text(55, 390, "Endogenous", 20) + text(251, 390, "1", 20, "#0861b5", 600)
  + text(55, 418, "Exogenous", 20) + text(251, 418, "1", 20, "#0861b5", 600)
  + text(55, 446, "Parameters", 20) + text(251, 446, "1", 20, "#0861b5", 600)
  + text(55, 474, "Equations", 20) + text(251, 474, "1", 20, "#0861b5", 600)
  + line(17, 490, 290, 490)
  + text(34, 517, "DYNARE PROJECT CHECKS", 16, "#516079", 600)
  + chevron(36, 535) + text(55, 545, "models", 21)
  + check(53, 566) + text(76, 575, "model.mod", 19)
  + check(53, 596) + text(76, 605, "stoch.mod", 19)
  + check(53, 626) + text(76, 635, "policy.mod", 19);
const code = ["var y;", "varexo e;", "parameters rho;", "rho = 1/2;", "var y;", "", "model;", "  [name='output']", "  y = rho*y(-1) + e;", "end;"];
for (let index = 0; index < code.length; ++index) {
  const y = 140 + index * 37;
  if (index >= 6) workbench += rect(292, y - 28, 891, 37, "#e9f2fc");
  workbench += text(311, y, index + 1, 19, "#8390a3", 400, true)
    + text(363, y, code[index], 25, index < 3 || index === 6 || index === 9 ? "#164eb5" : "#223047", 400, true);
}
workbench += text(554, 251, "= 0.5", 20, "#637b71", 400, true)
  + text(363, 338, "Browse 1 equation", 19, "#536c86")
  + `<path d="M334 284a7 7 0 1 1 12 0l-2 4h-8zM337 292h6M338 295h4" stroke="#a96300" stroke-width="2" fill="#fff2bd"/>`
  + line(363, 293, 447, 293, "#ba7200", 2)
  + rect(772, 127, 387, 80, "#ffffff", "#82acd8", 5)
  + text(790, 159, "y · endogenous", 23, "#0861b5", 600)
  + text(790, 190, "Declared at model.mod:1", 19, "#52657d")
  + rect(752, 282, 401, 125, "white", "#bbc5d2", 5)
  + text(770, 313, "Quick Fix…", 22, "#223047", 600)
  + line(753, 327, 1152, 327)
  + text(772, 356, "Ignore this check (W031)", 21, "#0861b5")
  + text(772, 389, "Explain this check (W031)", 21, "#0861b5")
  + rect(292, 518, 891, 132, "#fbfcfe") + line(292, 518, 1183, 518)
  + text(312, 548, "PROBLEMS  1", 18, "#223047", 600)
  + `<path d="M321 571l11 19h-22zM321 577v6M321 586v1" stroke="#a96300" stroke-width="1.7"/>`
  + text(339, 588, "W031   Symbol y declared twice", 22, "#865313")
  + text(339, 621, "First declaration: model.mod:1", 19, "#52657d")
  + text(178, 672, "x  1     e  1     #  1", 17, "white", 400, true)
  + check(660, 664, "white") + text(684, 672, "Dynare project: 3/3 checked", 17, "white");
save("workbench", "Dynare editing and project checks", "Illustrated native editor, equation CodeLens, related diagnostic and Ignore/Explain actions, with distinct model counts and project coverage.", workbench);

let diff = chrome("Dygnosis model Diff");
diff += text(42, 140, "Model Diff", 29, "#223047", 600)
  + text(42, 177, "Before (active model): model.mod", 20)
  + text(42, 210, "After (selected model): stoch.mod", 20)
  + button(1016, 118, 130, "Refresh", true)
  + text(42, 258, "Search", 18) + rect(108, 231, 247, 37, "white", "#bbc5d2", 4)
  + text(123, 256, "Names, values, or equations", 17, "#637287")
  + text(381, 258, "Scope", 18) + button(443, 231, 174, "All scopes  ⌄")
  + text(639, 258, "Layout", 18) + button(707, 231, 196, "Side by side  ⌄")
  + text(42, 302, "Change kinds", 18, "#516079", 600)
  + rect(207, 286, 17, 17, "white", "#52657d") + check(209, 294, "#0861b5") + text(233, 302, "Added", 20)
  + rect(324, 286, 17, 17, "white", "#52657d") + check(326, 294, "#0861b5") + text(350, 302, "Removed", 20)
  + rect(469, 286, 17, 17, "white", "#52657d") + check(471, 294, "#0861b5") + text(495, 302, "Changed", 20)
  + rect(614, 286, 17, 17, "white", "#52657d") + check(616, 294, "#0861b5") + text(640, 302, "Unpaired", 20)
  + text(42, 340, "2 of 2 rows shown · 2 changed", 20, "#52657d")
  + rect(40, 362, 1118, 128, "#f8fafc", "#c8d1dc", 5)
  + chevron(58, 379) + text(77, 395, "Parameter values (1)", 23, "#223047", 600)
  + text(68, 431, "rho", 23, "#0861b5", 600, true)
  + text(338, 424, "Before", 18, "#52657d", 600)
  + text(733, 424, "After", 18, "#52657d", 600)
  + text(338, 463, "0.5", 26, "#865313", 400, true)
  + text(733, 463, "0.75", 26, "#16724e", 400, true)
  + text(941, 463, "Open after ↗", 19, "#0861b5")
  + rect(40, 507, 1118, 128, "#f8fafc", "#c8d1dc", 5)
  + chevron(58, 524) + text(77, 540, "Aggregate equations (1)", 23, "#223047", 600)
  + text(68, 581, "output", 21, "#0861b5", 600)
  + text(338, 577, "Before", 18, "#52657d", 600)
  + text(733, 577, "After", 18, "#52657d", 600)
  + text(338, 615, "y = rho*y(-1) + e;", 23, "#865313", 400, true)
  + text(733, 615, "y = rho*y(-1) + 2*e;", 23, "#16724e", 400, true);
save("diff", "Structural model Diff", "Illustration of the engine's paired parameter and equation changes, native filters and verified source actions.", diff);

let origins = chrome("model.mod · effective preview", "equations.inc · written source");
origins += rect(17, 101, 581, 549, "#f8fafc") + line(598, 100, 598, 650)
  + text(42, 141, "Read-only effective model", 24, "#223047", 600)
  + text(43, 190, "var y1 y2;", 26, "#164eb5", 400, true)
  + text(43, 234, "model;", 26, "#164eb5", 400, true)
  + text(43, 278, "  y1 = 0;", 26, "#223047", 400, true)
  + rect(30, 296, 549, 46, "#dceafb")
  + text(43, 326, "  y2 = 0;", 26, "#223047", 400, true)
  + text(43, 370, "end;", 26, "#164eb5", 400, true)
  + rect(42, 405, 488, 123, "white", "#bbc5d2", 5)
  + rect(43, 406, 486, 41, "#0861b5", "none", 4)
  + text(58, 435, "Go to written source", 21, "white")
  + text(58, 475, "Show macro origins", 21, "#223047")
  + text(58, 514, "Refresh", 21, "#223047")
  + text(43, 567, "Trace the selected expanded equation", 21, "#52657d")
  + text(43, 602, "to the file you edit.", 21, "#52657d")
  + text(624, 141, "Written include", 24, "#223047", 600)
  + text(624, 197, "@#for i in [1,2]", 26, "#865313", 400, true)
  + rect(613, 218, 552, 46, "#dceafb")
  + text(624, 248, "y@{i} = 0;", 26, "#223047", 400, true)
  + text(624, 300, "@#endfor", 26, "#865313", 400, true)
  + rect(618, 379, 547, 158, "white", "#82acd8", 5)
  + text(638, 413, "Choose a verified macro origin", 22, "#223047", 600)
  + line(619, 430, 1164, 430)
  + text(638, 463, "for · i=2 · directive", 22, "#0861b5", 600)
  + text(638, 505, "equations.inc:1", 20, "#52657d")
  + text(625, 592, "Repeated copies retain their own origins.", 21, "#52657d");
save("origins", "Effective-model source and macro origins", "Illustration of a read-only expanded equation jumping to its exact written include, with a distinct macro iteration origin.", origins);
