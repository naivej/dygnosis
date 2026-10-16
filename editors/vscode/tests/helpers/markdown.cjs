// Model the MarkdownString API for status hover unit tests.
class MarkdownString {
  constructor(value = "") { this.value = value; }
  appendMarkdown(value) { this.value += value; return this; }
  appendText(value) { this.value += value.replace(/([\\`*_{}[\]()#+\-.!])/g, "\\$1"); return this; }
}
function tooltipText(value) {
  if (typeof value === "string") return value;
  return value.value.replace(/\\([\\`*_{}[\]()#+\-.!])/g, "$1").replaceAll("**", "").replaceAll("  \n", "\n").replaceAll("\n\n", "\n");
}
module.exports = { MarkdownString, tooltipText };
