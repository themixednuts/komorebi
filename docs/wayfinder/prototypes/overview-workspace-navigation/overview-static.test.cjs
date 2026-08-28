"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");

const html = fs.readFileSync("overview-workspace-navigation-prototype.html", "utf8");
const script = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
assert.ok(script, "inline interaction script exists");
assert.doesNotThrow(() => new Function(script), "inline interaction script parses");
for (const variant of ["spatial", "focus", "familiar"]) {
  assert.match(html, new RegExp(`data-variant="${variant}"`));
}
for (const concept of ["Windows Desktop 2", "Scratchpad", "PROTECTED CONTENT", "data-drop-workspace"]) {
  assert.match(html, new RegExp(concept));
}

console.log("overview HTML: syntax and 7 structural checks passed");
