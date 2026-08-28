"use strict";

function createModel() {
  return {
    revision: 7,
    focusedWindow: "codex",
    windows: {
      codex: { id: "codex", title: "Wayfinder", app: "Codex" },
      chrome: { id: "chrome", title: "Windows API", app: "Chrome" },
      discord: { id: "discord", title: "General", app: "Discord" },
      ghidra: { id: "ghidra", title: "Decompiler", app: "Ghidra" },
      steam: { id: "steam", title: "Library", app: "Steam" },
      terminal: { id: "terminal", title: "Build", app: "Terminal" },
      music: { id: "music", title: "Now playing", app: "Music" },
    },
    workspaces: [
      { id: "build", name: "BUILD", monitor: "main", windows: ["codex", "chrome", "terminal"], stacks: [["codex", "chrome"]] },
      { id: "comms", name: "COMMS", monitor: "main", windows: ["discord"] },
      { id: "reverse", name: "REVERSE", monitor: "side", windows: ["ghidra"] },
      { id: "play", name: "PLAY", monitor: "side", windows: ["steam"] },
    ],
    activeByMonitor: { main: "build", side: "reverse" },
    scratchpad: ["music"],
  };
}

function focusWindow(model, windowId) {
  if (!model.windows[windowId]) throw new Error("window-gone");
  model.focusedWindow = windowId;
  return { kind: "focus-window", windowId };
}

function focusWorkspace(model, monitorId, workspaceId) {
  const workspace = model.workspaces.find(item => item.id === workspaceId && item.monitor === monitorId);
  if (!workspace) throw new Error("workspace-gone");
  model.activeByMonitor[monitorId] = workspaceId;
  return { kind: "focus-workspace", monitorId, workspaceId };
}

function moveWindow(model, request) {
  if (request.expectedRevision !== model.revision) throw new Error("stale-revision");
  if (!model.windows[request.windowId]) throw new Error("window-gone");
  const target = model.workspaces.find(item => item.id === request.workspaceId);
  if (!target) throw new Error("workspace-gone");
  for (const workspace of model.workspaces) workspace.windows = workspace.windows.filter(id => id !== request.windowId);
  target.windows.push(request.windowId);
  model.revision += 1;
  return { kind: "move-window", revision: model.revision, target: target.id };
}

module.exports = { createModel, focusWindow, focusWorkspace, moveWindow };
