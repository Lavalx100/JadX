import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { JSDOM } from "jsdom";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const evernoteRoot = path.resolve(process.argv[2] ?? process.env.JADX_EVERNOTE_WORKSPACE ?? "");
if (!process.argv[2] && !process.env.JADX_EVERNOTE_WORKSPACE) {
  console.error("Usage: npm run eval:offline -- /path/to/evernote-workspace");
  process.exit(2);
}

function run(label, command, args, cwd) {
  const started = performance.now();
  const result = spawnSync(command, args, { cwd, encoding: "utf8" });
  return {
    label,
    status: result.status === 0 ? "passed" : "failed",
    duration_ms: Math.round(performance.now() - started),
    output: (result.stdout + result.stderr).trim().slice(-8_000),
  };
}

async function evaluateEvernoteWorkspace(workspace) {
  const html = await readFile(path.join(workspace, "index.html"), "utf8");
  const source = await readFile(path.join(workspace, "app.js"), "utf8");
  const open = (stored = {}) => {
    const dom = new JSDOM(html, { url: "http://localhost/", runScripts: "outside-only", pretendToBeVisual: true });
    for (const [key, value] of Object.entries(stored)) dom.window.localStorage.setItem(key, value);
    dom.window.eval(source);
    return dom;
  };
  const app = open();
  const document = app.window.document;
  assert.ok(document.querySelector("#noteList .note"), "seed notes render on first launch");

  document.getElementById("newNoteBtn").click();
  const title = document.getElementById("titleInput");
  const body = document.getElementById("bodyInput");
  const tags = document.getElementById("tagsInput");
  title.value = "Offline evaluation note";
  title.dispatchEvent(new app.window.Event("input", { bubbles: true }));
  body.value = "Notes can be created and edited.";
  body.dispatchEvent(new app.window.Event("input", { bubbles: true }));
  tags.value = "eval, notes";
  tags.dispatchEvent(new app.window.Event("input", { bubbles: true }));
  const notes = JSON.parse(app.window.localStorage.getItem("evernote.notes.v1"));
  assert.ok(notes.some((note) => note.title === "Offline evaluation note" && note.tags.includes("eval")));

  const stored = Object.fromEntries(
    Array.from({ length: app.window.localStorage.length }, (_, index) => {
      const key = app.window.localStorage.key(index);
      return [key, app.window.localStorage.getItem(key)];
    }),
  );
  app.window.close();
  const reopened = open(stored);
  const reopenedDocument = reopened.window.document;
  assert.equal(reopenedDocument.getElementById("titleInput").value, "Offline evaluation note", "selection and note survive reload");
  const search = reopenedDocument.getElementById("searchInput");
  search.value = "Offline evaluation note";
  search.dispatchEvent(new reopened.window.Event("input", { bubbles: true }));
  assert.equal(reopenedDocument.querySelectorAll("#noteList .note").length, 1, "search narrows the note list");

  reopenedDocument.querySelector('[data-view="tasks"]').click();
  const taskInput = reopenedDocument.querySelector(".task-form__input");
  taskInput.value = "Verify offline workflow";
  reopenedDocument.getElementById("taskForm").dispatchEvent(new reopened.window.Event("submit", { bubbles: true, cancelable: true }));
  assert.ok(JSON.parse(reopened.window.localStorage.getItem("evernote.tasks.v1")).some((task) => task.title === "Verify offline workflow"));
  reopened.window.close();
  return { label: "Evernote create/edit/search/reload/tasks", status: "passed" };
}

const results = [];
results.push(run("multi-file change-set + index + diagnostics", "cargo", ["test", "--manifest-path", "src-tauri/Cargo.toml", "offline_eval", "--", "--quiet"], root));
results.push(run("frontend type-check and bundle", "npm", ["run", "build"], root));
results.push(run("IDE validation fixture", "node", ["--test", "fixtures/ide-intelligence-validation/stats.test.js"], root));
try {
  results.push(await evaluateEvernoteWorkspace(evernoteRoot));
} catch (error) {
  results.push({ label: "Evernote create/edit/search/reload/tasks", status: "failed", error: String(error) });
}

const report = {
  mode: "offline; no inference provider called",
  compile: results.slice(0, 2).every((result) => result.status === "passed") ? "passed" : "failed",
  repair_attempts: 0,
  model_tokens: { input: 0, output: 0 },
  scenarios: results,
};
console.log(JSON.stringify(report, null, 2));
if (results.some((result) => result.status !== "passed")) process.exitCode = 1;
