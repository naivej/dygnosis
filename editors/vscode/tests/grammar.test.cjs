const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const { Registry, parseRawGrammar } = require("vscode-textmate");
const { loadWASM, OnigScanner, OnigString } = require("vscode-oniguruma");

const grammarPath = path.join(__dirname, "../syntaxes/dynare.tmLanguage.json");
const ready = (async () => {
  const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm"));
  await loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const registry = new Registry({
    onigLib: Promise.resolve({ createOnigScanner: patterns => new OnigScanner(patterns), createOnigString: value => new OnigString(value) }),
    loadGrammar: async scope => scope === "source.dynare" ? parseRawGrammar(fs.readFileSync(grammarPath, "utf8"), grammarPath) : null,
  });
  return registry.loadGrammar("source.dynare");
})();

async function tokenize(source) {
  const grammar = await ready;
  let ruleStack;
  return source.split("\n").map(text => {
    const result = grammar.tokenizeLine(text, ruleStack);
    ruleStack = result.ruleStack;
    return { text, tokens: result.tokens };
  });
}
function scopes(rows, line, needle) {
  const row = rows[line];
  const offset = row.text.indexOf(needle);
  assert.ok(offset >= 0, `missing ${needle} in ${row.text}`);
  return row.tokens.find(token => token.startIndex <= offset && offset < token.endIndex)?.scopes ?? [];
}
function has(rows, line, needle, scope) {
  assert.ok(scopes(rows, line, needle).includes(scope), `${needle}: ${scopes(rows, line, needle).join(" ")}`);
}

// Guards actual TextMate consumption of the shared fixed-head contract. Regex
// membership alone does not show that the pattern can be reached in the grammar.
test("shared command, block and declaration heads reach keyword scopes", async () => {
  const records = require("../../../src/engine/catalog/syntax_data.json");
  for (const record of records.filter(row => ["command", "block", "declaration"].includes(row.kind))) {
    for (const spelling of new Set([record.name, ...record.aliases])) {
      if (!/^[a-z_][a-z0-9_]*$/i.test(spelling)) continue;
      const qualified = record.signatures.length && record.signatures.every(signature => /\s\.\s(?:prior|options|subsamples)\b/i.test(signature));
      const rows = await tokenize(qualified ? `${spelling.toUpperCase()}(e,u).prior(shape=normal);` : `/* leading comment */ ${spelling.toUpperCase()};\nend;`);
      has(rows, 0, spelling.toUpperCase(), "keyword.control.dynare");
    }
  }
});

test("PAC and VAR heads, model functions, dates and D exponents have distinct roles", async () => {
  const rows = await tokenize("pac_model(name=pac,discount=betta);\nvar_model(name=v);\nmodel;\ny=log(x)+diff(x)+STEADY_STATE(x)+EXPECTATION(-1)(x)+SUM(h,x);\ny=1d3+1.D-2+.2e3;\nend;\nset_time(2020Q1);");
  has(rows, 0, "pac_model", "keyword.control.dynare");
  has(rows, 1, "var_model", "keyword.control.dynare");
  for (const name of ["log", "diff", "STEADY_STATE", "EXPECTATION", "SUM"]) has(rows, 3, name, "support.function.builtin.dynare");
  for (const value of ["1d3", "1.D-2", ".2e3"]) has(rows, 4, value, "constant.numeric.dynare");
  has(rows, 6, "2020Q1", "constant.other.date.dynare");
});

test("macro expressions use macro functions in directives and interpolation", async () => {
  const rows = await tokenize('@#define z = floor(log10(10))\nmodel;\ny=@{length([1,2])};\nend;');
  has(rows, 0, "@#define", "keyword.control.macro.dynare");
  for (const name of ["floor", "log10"]) has(rows, 0, name, "support.function.builtin.macro.dynare");
  has(rows, 2, "length", "support.function.builtin.macro.dynare");
});

test("native text, continued commands, strings and comments retain their boundaries", async () => {
  const rows = await tokenize('disp(log(x));\ndisp ...   \n log(x)\nverbatim;\npac_model; log(x);\nend;\nmodel;\n// log(x)\ny=log(x); label="var_model";\nend;');
  for (const [line, needle] of [[0, "log"], [2, "log"], [4, "pac_model"], [4, "log"]]) {
    assert.ok(!scopes(rows, line, needle).some(scope => scope === "support.function.builtin.dynare" || scope === "keyword.control.dynare"));
  }
  has(rows, 7, "log", "comment.line.double-slash.dynare");
  has(rows, 8, "var_model", "string.quoted.double.dynare");
  has(rows, 8, "log", "support.function.builtin.dynare");
});

test("open assignments color shared expression syntax without claiming Dynare builtins", async () => {
  // A static grammar cannot prove that an assignment head is declared. Both
  // accepted parameter assignments and native calls need usable fallback
  // colors, while semantic bindings remain the parser's responsibility.
  const rows = await tokenize("parameters p;\np=log(2)+1d3;\nout=pac_model(1)+diff(2)+learnt_in(3);\np=1!=2; p=1<=2; p=1>=2; p=1==2;\np=Inf+NaN;\n");
  has(rows, 1, "log", "support.function.matlab");
  has(rows, 1, "=", "keyword.operator.matlab");
  for (const number of ["2", "1d3"]) has(rows, 1, number, "constant.numeric.matlab");
  for (const name of ["pac_model", "diff", "learnt_in"]) {
    has(rows, 2, name, "support.function.matlab");
    assert.ok(!scopes(rows, 2, name).some(scope => scope === "support.function.builtin.dynare" || scope === "keyword.control.dynare"));
  }
  for (const operator of ["!=", "<=", ">=", "=="]) {
    const offset = rows[3].text.indexOf(operator);
    const token = rows[3].tokens.find(token => token.startIndex <= offset && offset < token.endIndex);
    assert.ok(token?.scopes.includes("keyword.operator.matlab") && token.endIndex >= offset + operator.length, `whole ${operator} operator`);
  }
  for (const literal of ["Inf", "NaN"]) has(rows, 4, literal, "constant.language.matlab");
});

test("matrix separators and terminal-period end do not close their construct", async () => {
  const rows = await tokenize('stoch_simul(irf_shocks=[e;u],irf=20);\nshock_paths;\nperiods 1:end;\nvalues log(p);\nend;\nsteady;');
  has(rows, 0, "irf=", "variable.parameter.option.dynare");
  has(rows, 3, "log", "support.function.builtin.dynare");
  has(rows, 5, "steady", "keyword.control.dynare");
});

test("qualified option owners keep subsamples and native wrappers separate", async () => {
  const rows = await tokenize("std(e).early.prior(shape=normal,mean=0,stdev=1);\ncorr(e,u).options(jscale=.2);\na.subsamples(early=2000Q1:2001Q1);\nstd(x); corr(x);\na.prior(mean=log(p));");
  for (const name of ["std", "prior"]) has(rows, 0, name, "keyword.control.dynare");
  for (const name of ["shape", "mean", "stdev"]) has(rows, 0, name, "variable.parameter.option.dynare");
  has(rows, 1, "jscale", "variable.parameter.option.dynare");
  has(rows, 2, "subsamples", "keyword.control.dynare");
  has(rows, 2, "2000Q1", "constant.other.date.dynare");
  for (const name of ["std", "corr"]) assert.ok(!scopes(rows, 3, name).includes("keyword.control.dynare"));
  has(rows, 4, "mean", "variable.parameter.option.dynare");
  has(rows, 4, "log", "support.function.builtin.dynare");
});

test("model-only functions do not acquire ordinary expression roles", async () => {
  const rows = await tokenize("initval;\ny=diff(p)+EXPECTATION(0)(p)+log(p);\nend;\nsteady_state_model;\ny=diff(p)+log(p);\nend;\nplanner_objective diff(y)+log(y);\nvar(deflator=diff(p)+log(p)) y;");
  for (const [line, name] of [[1, "diff"], [1, "EXPECTATION"], [4, "diff"]]) assert.ok(!scopes(rows, line, name).includes("support.function.builtin.dynare"));
  for (const line of [1, 4, 6, 7]) has(rows, line, "log", "support.function.builtin.dynare");
  for (const line of [6, 7]) has(rows, line, "diff", "support.function.builtin.dynare");
});

test("finite option, body and function records are reachable in their grammar context", async () => {
  // This is the consumed token-role contract; each option uses its actual
  // owner, so a generated but unreachable rule fails here.
  const records = require("../../../src/engine/catalog/syntax_data.json");
  const heads = new Map(records.filter(row => ["command", "block", "declaration"].includes(row.kind)).map(row => [row.name, row]));
  for (const record of records) {
    if (!/^[a-z_][a-z0-9_]*$/i.test(record.name)) continue;
    if (record.kind === "option") {
      const head = heads.has(record.parent) ? record.parent : `p.${record.parent}`;
      const rows = await tokenize(`${head}(${record.name}=0);\nend;`);
      const point = head.length + 1;
      const token = rows[0].tokens.find(token => token.startIndex <= point && point < token.endIndex);
      assert.ok(token?.scopes.includes("variable.parameter.option.dynare"), `${head}.${record.name}: ${token?.scopes.join(" ")}`);
    } else if (record.kind === "body_keyword") {
      assert.ok(heads.has(record.parent), `missing body owner ${record.parent}`);
      const rows = await tokenize(`${record.parent};\n${record.name} 1;\nend;`);
      has(rows, 1, record.name, "keyword.control.dynare");
    } else if (record.kind === "function" || (record.kind === "operator" && record.contexts.includes("model_expression"))) {
      const parent = record.contexts.includes("model_expression") ? "model" : "initval";
      const rows = await tokenize(`${parent};\ny=${record.name}(1);\nend;`);
      has(rows, 1, record.name, "support.function.builtin.dynare");
    } else if (record.kind === "macro_function") {
      const cast = record.signatures.some(signature => signature.startsWith(`(${record.name})`));
      const rows = await tokenize(`@#define x = ${cast ? `(${record.name})1` : `${record.name}(1)`}`);
      has(rows, 0, record.name, cast ? "storage.type.macro.dynare" : "support.function.builtin.macro.dynare");
    }
  }
});
