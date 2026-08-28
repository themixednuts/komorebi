"use strict";

const assert = require("node:assert/strict");
const { createModel, focusWindow, focusWorkspace, moveWindow } = require("./overview-model.cjs");

const model = createModel();
assert.deepEqual(focusWindow(model, "chrome"), { kind: "focus-window", windowId: "chrome" });
assert.equal(model.focusedWindow, "chrome");
assert.deepEqual(focusWorkspace(model, "side", "play"), { kind: "focus-workspace", monitorId: "side", workspaceId: "play" });
assert.equal(model.activeByMonitor.side, "play");

const moved = moveWindow(model, { windowId: "chrome", workspaceId: "comms", expectedRevision: 7 });
assert.deepEqual(moved, { kind: "move-window", revision: 8, target: "comms" });
assert.equal(model.workspaces.find(item => item.id === "build").windows.includes("chrome"), false);
assert.equal(model.workspaces.find(item => item.id === "comms").windows.includes("chrome"), true);
assert.throws(() => moveWindow(model, { windowId: "codex", workspaceId: "play", expectedRevision: 7 }), /stale-revision/);
assert.throws(() => focusWindow(model, "gone"), /window-gone/);
assert.throws(() => focusWorkspace(model, "main", "gone"), /workspace-gone/);

console.log("overview model: 9 checks passed");
