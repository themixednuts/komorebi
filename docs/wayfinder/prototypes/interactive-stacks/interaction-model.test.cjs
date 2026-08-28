const fs = require("fs");

class ClassList {
  constructor() { this.values = new Set(); }
  add(...values) { values.forEach(value => this.values.add(value)); }
  remove(...values) { values.forEach(value => this.values.delete(value)); }
  toggle(value, force) {
    if (force === true) this.values.add(value);
    else if (force === false) this.values.delete(value);
    else if (this.values.has(value)) this.values.delete(value);
    else this.values.add(value);
  }
}

class Element {
  constructor(dataset = {}) {
    this.dataset = dataset;
    this.classList = new ClassList();
    this.style = {};
    this.attributes = {};
    this.innerHTML = "";
    this.textContent = "";
    this.tagName = "DIV";
  }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener() {}
  closest() { return null; }
}

const ids = new Map([
  "#monitors", "#ghost", "#status", "#toast", "#variant-title", "#variant-copy",
  "#read-window", "#read-target", "#read-state", "#read-revision", "#event-log",
  "#stale", "#reset", "#keyboard-place"
].map(selector => [selector, new Element()]));

const switchers = ["direct", "rail", "compass"].map(value => new Element({ variantSwitch: value }));
const documentStub = {
  body: new Element(),
  activeElement: null,
  querySelector(selector) { return ids.get(selector) || new Element(); },
  querySelectorAll(selector) { return selector === "[data-variant-switch]" ? switchers : []; },
  addEventListener() {},
  elementFromPoint() { return null; }
};

const html = fs.readFileSync(__dirname + "/interactive-stacks-prototype.html", "utf8");
const scriptStart = html.indexOf("<script>") + 8;
const scriptEnd = html.lastIndexOf("</script>");
if (scriptStart < 8 || scriptEnd < scriptStart) throw new Error("prototype script not found");

const checks = `
  function topology() {
    return JSON.stringify({ revision: model.revision, containers: model.containers });
  }
  function resetModel() {
    model = seed();
    drag = null;
    document.body.classList.remove("dragging");
  }
  function assert(condition, message) {
    if (!condition) throw new Error(message);
    console.log("PASS " + message);
  }

  resetModel();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c3", action: "stack" });
  commitDrag();
  assert(containerForWindow("docs").id === "c3", "center target stacks into the exact container");
  assert(model.revision === 2, "valid drop commits one revision");

  resetModel();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c1", action: "reorder", before: "browser" });
  commitDrag();
  assert(model.containers.find(item => item.id === "c1").windows.join(",") === "docs,browser", "stackbar slot performs exact tab reorder");

  resetModel();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c3", action: "right" });
  commitDrag();
  assert(containerForWindow("docs").windows.length === 1 && containerForWindow("docs").rect[0] === 75, "edge target creates the requested split side");

  resetModel();
  const beforeLocked = topology();
  beginDrag("browser", "pointer");
  setCandidate({ containerId: "c2", action: "stack" });
  commitDrag();
  assert(topology() === beforeLocked, "locked group rejects membership change without mutation");

  resetModel();
  const beforeLockedSource = topology();
  const lockedStarted = beginDrag("terminal", "pointer");
  assert(lockedStarted === false && topology() === beforeLockedSource, "locked source cannot begin a structural placement");

  resetModel();
  const beforeCancel = topology();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c3", action: "left" });
  cancelDrag();
  assert(topology() === beforeCancel, "Escape-equivalent cancellation preserves topology and revision");

  resetModel();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c4", action: "stack" });
  commitDrag();
  assert(containerForWindow("docs").monitor === 1, "cross-monitor drop uses the same semantic target");

  resetModel();
  model.staleNextCommit = true;
  const beforeStale = topology();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c3", action: "stack" });
  commitDrag();
  assert(topology() === beforeStale, "stale target rejection preserves source topology");

  resetModel();
  const beforeRevisionRace = topology();
  beginDrag("docs", "pointer");
  setCandidate({ containerId: "c3", action: "stack" });
  model.revision += 1;
  commitDrag();
  assert(JSON.stringify(model.containers) === JSON.stringify(JSON.parse(beforeRevisionRace).containers) && model.revision === 2, "revision race rejects topology change");

  resetModel();
  beginDrag("docs", "keyboard");
  setCandidate({ containerId: "c3", action: "bottom" });
  commitDrag();
  assert(containerForWindow("docs").rect[1] === 75, "keyboard path commits through the same split intent");
`;

const run = new Function(
  "document", "location", "history", "setTimeout", "clearTimeout",
  html.slice(scriptStart, scriptEnd) + checks
);

run(
  documentStub,
  { href: "file:///prototype.html?variant=direct", search: "?variant=direct" },
  { replaceState() {} },
  () => 0,
  () => {}
);
