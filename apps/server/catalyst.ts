import { promises as fs } from "node:fs";
import path from "node:path";
import { createHash, randomUUID } from "node:crypto";
import { spawn, type ChildProcess } from "node:child_process";
import { z } from "zod";
import { command } from "./media.ts";
import { root, writeJson, readJson } from "./storage.ts";
import { topicSettings } from "./topics.ts";
import { speechStatus } from "./whisper.ts";

export interface BuildTask {
  id: string;
  title: string;
  acceptance: string;
  lane: "Now" | "Next" | "Later";
  status: "todo" | "doing" | "review" | "done";
  evidence: string[];
}
export interface Pipeline {
  name: string;
  milestone: string;
  updatedAt: string;
  message: string;
  tasks: BuildTask[];
}
export interface Proposal {
  id: string;
  path: string;
  summary: string;
  before: string;
  after: string;
  hash: string;
  status: "review" | "applying" | "applied" | "discarded";
  createdAt: string;
  commit?: string;
  undoOf?: string;
}
export interface CheckRun {
  id: string;
  kind: "build" | "test";
  status: "running" | "passed" | "failed" | "cancelled";
  startedAt: string;
  endedAt?: string;
  output: string;
  exitCode?: number;
}
const digest = (s: string) => createHash("sha256").update(s).digest("hex");
export const privateDir = path.join(root, ".catalyst", "private");
const editableRoots = ["apps", "packages", "docs", "scripts", "tests"];
export async function safeFile(relative: string, base = root) {
  if (
    !relative ||
    relative.includes("\\") ||
    path.isAbsolute(relative) ||
    relative
      .split("/")
      .some(
        (p) =>
          !p ||
          p === "." ||
          p === ".." ||
          p.startsWith(".env") ||
          p === "node_modules" ||
          p === "dist" ||
          p.startsWith(".git"),
      )
  )
    throw new Error("That file path is not available in Catalyst.");
  const top = relative.split("/")[0];
  if (
    !editableRoots.includes(top) &&
    ![
      "README.md",
      "BUILD_PROMPT.md",
      "package.json",
      "tsconfig.json",
      "eslint.config.js",
      ".catalyst/pipeline.json",
      ".catalyst/instructions.md",
    ].includes(relative)
  )
    throw new Error(
      "Catalyst only edits this project’s source, documentation and build instructions.",
    );
  if (
    !/\.(tsx?|jsx?|mjs|css|html|json|md|svg|ya?ml)$/.test(relative) ||
    relative.split("/").some((p) => /credential|secret|\.pem$|\.key$/i.test(p))
  )
    throw new Error("Only non-private text source files can be opened.");
  const parts = relative.split("/");
  let resolved = base;
  for (const part of parts) {
    resolved = path.join(resolved, part);
    const stat = await fs.lstat(resolved);
    if (stat.isSymbolicLink())
      throw new Error("Symlinks are not available in Catalyst.");
  }
  if ((await fs.stat(resolved)).size > 512000)
    throw new Error("This file is too large for the source editor.");
  return resolved;
}
export function secretScan(text: string) {
  if (
    /-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----|\bsk-(?:proj-|ant-)?[A-Za-z0-9_-]{24,}|\bgh[pousr]_[A-Za-z0-9]{30,}|\bAKIA[0-9A-Z]{16}\b|(?:api[_-]?key|password|token|secret)\s*[:=]\s*["'][A-Za-z0-9_+/-]{20,}["']/i.test(
      text,
    )
  )
    throw new Error(
      "This change looks like it contains a secret. Move credentials to your private environment before saving.",
    );
}
export async function listFiles(base = root) {
  const results: string[] = [];
  async function walk(folder: string) {
    for (const item of await fs
      .readdir(path.join(base, folder), { withFileTypes: true })
      .catch(() => [])) {
      if (
        item.isSymbolicLink() ||
        item.name.startsWith(".") ||
        ["node_modules", "dist", "public"].includes(item.name)
      )
        continue;
      const file = folder + "/" + item.name;
      if (item.isDirectory()) await walk(file);
      else if (/\.(tsx?|jsx?|mjs|css|html|json|md|svg|ya?ml)$/.test(file)) {
        try {
          await safeFile(file, base);
          results.push(file);
        } catch {
          /* private or oversized */
        }
      }
    }
  }
  for (const folder of editableRoots) await walk(folder);
  for (const file of [
    "README.md",
    "BUILD_PROMPT.md",
    "package.json",
    "tsconfig.json",
    "eslint.config.js",
    ".catalyst/pipeline.json",
    ".catalyst/instructions.md",
  ]) {
    try {
      await safeFile(file, base);
      results.push(file);
    } catch {
      /* optional file */
    }
  }
  return results.sort();
}
async function git(
  args: string[],
  base = root,
  env: NodeJS.ProcessEnv = process.env,
) {
  return new Promise<string>((resolve, reject) => {
    const p = spawn("git", ["--literal-pathspecs", ...args], {
      cwd: base,
      env,
      windowsHide: true,
    });
    let out = "",
      err = "";
    p.stdout.on("data", (b) => (out += b.toString()));
    p.stderr.on("data", (b) => (err += b.toString()));
    p.on("error", reject);
    p.on("close", (code) =>
      code === 0
        ? resolve(out)
        : reject(new Error(err.trim() || "Git operation failed.")),
    );
  });
}
export async function scanSite() {
  const files = await listFiles();
  const packageJson = await readJson<{ dependencies?: Record<string, string> }>(
    path.join(root, "package.json"),
  );
  const site = {
    name: "Storyfold",
    domain: "localhost",
    stack: packageJson.dependencies?.react
      ? "React + TypeScript / Vite / Express"
      : "Unknown",
    pages: [
      {
        id: "editor",
        name: "Storyfold editor",
        route: "/",
        title: "Storyfold — find the story",
        desc: "Edit interviews by sentences and topics.",
        nav: true,
        file: "apps/web/src/Editor.tsx",
        secs: [
          {
            id: "workspace",
            code: "custom",
            variant: "custom",
            label: "Interview workspace",
            formula: "Application workspace",
            p: {},
          },
        ],
      },
      {
        id: "catalyst",
        name: "Catalyst Solo",
        route: "/catalyst",
        title: "Catalyst Solo — your build",
        desc: "See, control and edit the build.",
        nav: true,
        file: "apps/web/src/Catalyst.tsx",
        secs: [
          {
            id: "dashboard",
            code: "custom",
            variant: "custom",
            label: "Build control panel",
            formula: "Application dashboard",
            p: {},
          },
        ],
      },
    ],
    redirects: [],
    footerLinks: [],
    integrations: [
      {
        id: "transcription",
        name: "Speech transcription",
        via: (await speechStatus()).speechProvider,
        env: ["OPENAI_API_KEY"],
      },
      {
        id: "topics",
        name: "Topic folders",
        via: topicSettings().topicProvider,
        env: ["CLAUDE_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"],
      },
    ],
    scannedAt: new Date().toISOString(),
    files,
    notes: [
      "Application views are custom sections; marketing-site hero/header rules do not apply.",
      "Local development. No live deployment is configured.",
    ],
  };
  await writeJson(path.join(root, ".catalyst", "site.json"), site);
  return site;
}
export async function getPipeline() {
  return readJson<Pipeline>(path.join(root, ".catalyst", "pipeline.json"));
}
export const taskEdit = z.object({
  title: z.string().trim().min(1).max(160),
  acceptance: z.string().trim().min(1).max(2000),
  lane: z.enum(["Now", "Next", "Later"]),
  status: z.enum(["todo", "doing", "review", "done"]),
});
export async function changeTask(id: string, input: unknown) {
  const patch = taskEdit.parse(input),
    p = await getPipeline();
  const task = p.tasks.find((t) => t.id === id);
  if (!task) throw new Error("Task does not exist.");
  Object.assign(task, patch);
  p.updatedAt = new Date().toISOString();
  await writeJson(path.join(root, ".catalyst", "pipeline.json"), p);
  return p;
}
export async function addTask(input: unknown) {
  const patch = taskEdit.parse(input),
    p = await getPipeline();
  p.tasks.push({ ...patch, id: randomUUID(), evidence: [] });
  p.updatedAt = new Date().toISOString();
  await writeJson(path.join(root, ".catalyst", "pipeline.json"), p);
  return p;
}
export async function getControl() {
  return readJson<{ paused: boolean }>(
    path.join(root, ".catalyst", "control.json"),
  ).catch(() => ({ paused: false }));
}
export async function setControl(paused: boolean) {
  await writeJson(path.join(root, ".catalyst", "control.json"), { paused });
  return { paused };
}
const proposalPath = (id: string) => {
  if (!/^[\w-]{1,60}$/.test(id)) throw new Error("Invalid proposal.");
  return path.join(privateDir, "proposals", id + ".json");
};
export async function getProposals() {
  const names = await fs
    .readdir(path.join(privateDir, "proposals"))
    .catch(() => []);
  return (
    await Promise.all(
      names
        .filter((n) => n.endsWith(".json"))
        .map((n) => readJson<Proposal>(path.join(privateDir, "proposals", n))),
    )
  ).sort((a, b) => b.createdAt.localeCompare(a.createdAt));
}
export async function proposeChange(
  input: unknown,
  base = root,
  store = privateDir,
) {
  const req = z
    .object({
      path: z.string(),
      after: z.string().max(512000),
      summary: z.string().trim().min(1).max(200),
      hash: z.string(),
      undoOf: z.string().optional(),
    })
    .parse(input);
  const file = await safeFile(req.path, base);
  const before = await fs.readFile(file, "utf8");
  secretScan(before);
  secretScan(req.after);
  if (req.path.endsWith(".json")) {
    try {
      JSON.parse(req.after);
    } catch {
      throw new Error(
        "This JSON draft has a syntax error. Fix it before creating a review.",
      );
    }
  }
  if (digest(before) !== req.hash)
    throw Object.assign(
      new Error(
        "This file changed since you opened it. Reload before proposing an edit.",
      ),
      { status: 409 },
    );
  if (before === req.after) throw new Error("There are no changes to review.");
  const proposal: Proposal = {
    id: randomUUID(),
    path: req.path,
    summary: req.summary,
    before,
    after: req.after,
    hash: req.hash,
    status: "review",
    createdAt: new Date().toISOString(),
    undoOf: req.undoOf,
  };
  await writeJson(
    path.join(store, "proposals", proposal.id + ".json"),
    proposal,
  );
  return proposal;
}
export async function readSource(relative: string) {
  const content = await fs.readFile(await safeFile(relative), "utf8");
  secretScan(content);
  return { path: relative, content, hash: digest(content) };
}
let applying = false;
export async function applyProposal(
  id: string,
  base = root,
  store = privateDir,
) {
  if (applying)
    throw new Error("Another change is being saved. Try again in a moment.");
  const entry = path.join(store, "proposals", path.basename(proposalPath(id)));
  applying = true;
  let proposal: Proposal | undefined;
  let file = "";
  let written = false;
  const index = path.join(store, `index-${randomUUID()}`);
  try {
    proposal = await readJson<Proposal>(entry);
    if (proposal.status !== "review")
      throw new Error("This proposal is no longer waiting for review.");
    file = await safeFile(proposal.path, base);
    const current = await fs.readFile(file, "utf8");
    if (digest(current) !== proposal.hash)
      throw Object.assign(
        new Error(
          "The source changed after this review was created. Create a new review.",
        ),
        { status: 409 },
      );
    secretScan(proposal.after);
    if (
      (
        await git(
          ["diff", "--cached", "--name-only", "--", proposal.path],
          base,
        )
      ).trim()
    )
      throw new Error(
        "This file already has staged work. Commit or unstage that work before applying this proposal.",
      );
    const env = { ...process.env, GIT_INDEX_FILE: index };
    await git(["read-tree", "HEAD"], base, env);
    proposal.status = "applying";
    await writeJson(entry, proposal);
    await fs.writeFile(file, proposal.after);
    written = true;
    await git(["add", "--", proposal.path], base, env);
    await git(["commit", "-m", `catalyst: ${proposal.summary}`], base, env);
    proposal.commit = (await git(["rev-parse", "HEAD"], base)).trim();
    proposal.status = "applied";
    await writeJson(entry, proposal);
    await git(["reset", "-q", "HEAD", "--", proposal.path], base);
    return proposal;
  } catch (error) {
    if (written && proposal && proposal.status !== "applied") {
      if ((await fs.readFile(file, "utf8")) === proposal.after)
        await fs.writeFile(file, proposal.before);
      proposal.status = "review";
      await writeJson(entry, proposal);
    }
    throw error;
  } finally {
    await fs.rm(index, { force: true });
    applying = false;
  }
}
export async function discardProposal(id: string) {
  const file = proposalPath(id),
    p = await readJson<Proposal>(file);
  if (p.status !== "review")
    throw new Error("Only pending reviews can be discarded.");
  p.status = "discarded";
  await writeJson(file, p);
  return p;
}
export async function undoProposal(id: string) {
  const p = await readJson<Proposal>(proposalPath(id));
  if (p.status !== "applied")
    throw new Error("Only applied changes can be undone.");
  return proposeChange({
    path: p.path,
    after: p.before,
    hash: digest(p.after),
    summary: `Undo ${p.summary}`,
    undoOf: id,
  });
}
export async function history() {
  const log = await git(["log", "-20", "--format=%h%x09%aI%x09%s"]);
  const status = await git(["status", "--short"]);
  return {
    commits: log
      .trim()
      .split("\n")
      .filter(Boolean)
      .map((line) => {
        const [id, at, ...message] = line.split("\t");
        return { id, at, message: message.join("\t") };
      }),
    status,
  };
}
let processRun: ChildProcess | undefined;
let run: CheckRun | undefined;
export function activeRun() {
  return run;
}
export async function startCheck(kind: "build" | "test", changed: () => void) {
  if ((await getControl()).paused)
    throw new Error("The local pipeline is paused. Resume it to run checks.");
  if (processRun) throw new Error("Checks are already running.");
  run = {
    id: randomUUID(),
    kind,
    status: "running",
    startedAt: new Date().toISOString(),
    output: `$ npm run ${kind}\n`,
  };
  const state = run;
  const logPath = path.join(root, ".catalyst", "runs", state.id + ".json");
  await writeJson(logPath, state);
  processRun = spawn(
    process.platform === "win32" ? "npm.cmd" : "npm",
    ["run", kind],
    {
      cwd: root,
      detached: process.platform !== "win32",
      windowsHide: true,
      env: { ...process.env, FORCE_COLOR: "0" },
    },
  );
  const output = (b: Buffer) => {
    state.output = (state.output + b.toString()).slice(-100000);
    changed();
  };
  processRun.stdout?.on("data", output);
  processRun.stderr?.on("data", output);
  processRun.on("error", () => {
    state.output += "Could not start npm.\n";
  });
  processRun.on("close", (code) => {
    if (state.status === "running")
      state.status = code === 0 ? "passed" : "failed";
    state.exitCode = code ?? 1;
    state.endedAt = new Date().toISOString();
    processRun = undefined;
    void writeJson(logPath, state);
    changed();
  });
  changed();
  return state;
}
export function cancelCheck() {
  if (processRun && run) {
    run.status = "cancelled";
    if (process.platform !== "win32" && processRun.pid)
      process.kill(-processRun.pid, "SIGTERM");
    else processRun.kill("SIGTERM");
  }
  return run;
}
export async function recentRuns() {
  const dir = path.join(root, ".catalyst", "runs");
  const names = await fs.readdir(dir).catch(() => []);
  const saved = await Promise.all(
    names
      .filter((n) => n.endsWith(".json"))
      .map((n) => readJson<CheckRun>(path.join(dir, n))),
  );
  return saved
    .map((r) =>
      r.status === "running" && r.id !== run?.id
        ? {
            ...r,
            status: "cancelled" as const,
            output: r.output + "\nServer stopped before these checks finished.",
          }
        : r,
    )
    .sort((a, b) => b.startedAt.localeCompare(a.startedAt))
    .slice(0, 10);
}
export async function toolsStatus() {
  return {
    ffmpeg: await command("ffmpeg", ["-version"]).then(
      () => true,
      () => false,
    ),
    ffprobe: await command("ffprobe", ["-version"]).then(
      () => true,
      () => false,
    ),
    ...(await speechStatus()),
    ...topicSettings(),
  };
}
