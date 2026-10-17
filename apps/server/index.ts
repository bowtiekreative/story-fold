import "node:process";
import express, {
  type Request,
  type Response,
  type NextFunction,
} from "express";
import multer from "multer";
import path from "node:path";
import { promises as fs } from "node:fs";
import { randomUUID, randomBytes, timingSafeEqual } from "node:crypto";
import { createServer, type ServerResponse } from "node:http";
import { z, ZodError } from "zod";
import {
  root,
  dataRoot,
  readJson,
  writeJson,
  projectDir,
  readProject,
  saveProject,
  withProjectWrite,
} from "./storage.ts";
import { applyProjectEdit, settingsSchema, wordSchema } from "./model.ts";
import {
  processProject,
  createSample,
  renderExport,
  type ExportJob,
  type ProcessingInput,
} from "./media.ts";
import * as catalyst from "./catalyst.ts";
import { speechStatus } from "./whisper.ts";
import type { Project, CamId } from "../../packages/timeline/index.ts";

try {
  process.loadEnvFile(path.join(root, ".env"));
} catch {
  /* optional */
}
const token = randomBytes(32).toString("hex");
const app = express();
app.disable("x-powered-by");
app.use((req, res, next) => {
  const host = req.headers.host ?? "";
  if (!/^(localhost|127\.0\.0\.1|\[::1\])(?::\d+)?$/.test(host)) {
    res.status(403).json({ error: "This app is available only on localhost." });
    return;
  }
  const origin = req.headers.origin;
  if (origin) {
    try {
      const url = new URL(origin);
      if (!["localhost", "127.0.0.1", "[::1]"].includes(url.hostname))
        throw new Error();
    } catch {
      res.status(403).json({ error: "Cross-origin requests are not allowed." });
      return;
    }
  }
  res.setHeader("X-Content-Type-Options", "nosniff");
  res.setHeader("Referrer-Policy", "same-origin");
  res.setHeader("X-Frame-Options", "SAMEORIGIN");
  if (
    req.path.startsWith("/api") &&
    !["GET", "HEAD", "OPTIONS"].includes(req.method)
  ) {
    const supplied = Buffer.from(
      String(req.headers["x-storyfold-token"] ?? ""),
    );
    const expected = Buffer.from(token);
    if (
      supplied.length !== expected.length ||
      !timingSafeEqual(supplied, expected)
    ) {
      res
        .status(403)
        .json({ error: "Your local session expired. Reload this page." });
      return;
    }
  }
  next();
});
app.use(express.json({ limit: "5mb" }));
const listeners = new Set<ServerResponse>();
function broadcast() {
  for (const res of listeners) res.write("event: update\ndata: {}\n\n");
}
app.get("/api/session", (_req, res) => {
  res.setHeader("Cache-Control", "no-store");
  res.json({ token });
});
app.get("/api/events", (req, res) => {
  res.setHeader("Content-Type", "text/event-stream");
  res.setHeader("Cache-Control", "no-cache");
  res.setHeader("Connection", "keep-alive");
  res.flushHeaders();
  res.write("event: update\ndata: {}\n\n");
  listeners.add(res);
  const interval = setInterval(() => res.write(": heartbeat\n\n"), 15000);
  req.on("close", () => {
    clearInterval(interval);
    listeners.delete(res);
  });
});
app.get("/api/health", async (_req, res) =>
  res.json({ ok: true, ...(await catalyst.toolsStatus()) }),
);
app.get("/api/projects", async (_req, res) => {
  const dirs = await fs
    .readdir(path.join(dataRoot, "projects"), { withFileTypes: true })
    .catch(() => []);
  const projects = (
    await Promise.all(
      dirs
        .filter((d) => d.isDirectory())
        .map((d) => readProject(d.name).catch(() => null)),
    )
  ).filter((p): p is Project => p !== null);
  res.json(
    projects
      .sort((a, b) => b.createdAt.localeCompare(a.createdAt))
      .map((p) => ({
        id: p.id,
        name: p.name,
        status: p.status,
        createdAt: p.createdAt,
        sentences: p.sentences.length,
        sample: !!p.sample,
      })),
  );
});
app.get("/api/projects/:id", async (req, res) => {
  const p = await readProject(String(req.params.id));
  // Paths are generated identifiers only; raw upload names and filesystem roots stay on the server.
  res.json(p);
});
const pendingProjects = new Set<string>();
let queue = Promise.resolve();
function enqueue(project: Project, input: ProcessingInput, generate = false) {
  if (pendingProjects.has(project.id))
    throw new Error("This project is already being processed.");
  pendingProjects.add(project.id);
  queue = queue.then(async () => {
    try {
      if (generate) {
        project.progress.push({
          step: "uploading",
          detail: "Making a synthetic test video",
          pct: 1,
          at: new Date().toISOString(),
        });
        await saveProject(project);
        broadcast();
        input.words = await createSample(project);
        await writeJson(path.join(projectDir(project.id), "input.json"), input);
      }
      await processProject(project, input, broadcast);
    } catch (error) {
      project.status = "error";
      project.error =
        error instanceof Error ? error.message : "Processing failed";
      await saveProject(project);
      broadcast();
    } finally {
      pendingProjects.delete(project.id);
    }
  });
}
function newProject(name: string): Project {
  return {
    id: randomUUID(),
    name,
    revision: 0,
    createdAt: new Date().toISOString(),
    status: "uploading",
    cameras: [],
    sentences: [],
    topics: [],
    timeline: [],
    exportSettings: { ratio: "16:9", captions: true, chapters: true },
    progress: [],
    warnings: [],
  };
}
app.post("/api/projects/sample", async (_req, res) => {
  const project = newProject("The bakery · sample story");
  project.sample = true;
  project.warnings = [
    "Synthetic test video and supplied text. The tone is not speech. Use your interview to validate transcription and sync.",
  ];
  project.cameras = [
    {
      id: "A",
      name: "Wide",
      sourcePath: "source-A.mp4",
      proxyPath: "proxy-A.mp4",
      offset: 0,
      duration: 36,
    },
  ];
  await saveProject(project);
  enqueue(project, { manual: true, sample: true }, true);
  res.status(202).json({ projectId: project.id });
});
const uploadDir = path.join(dataRoot, "uploads");
await fs.mkdir(uploadDir, { recursive: true });
const upload = multer({
  dest: uploadDir,
  limits: {
    files: 4,
    fileSize: 4 * 1024 * 1024 * 1024,
    fieldSize: 1000000,
    fields: 4,
  },
}).fields([
  { name: "files", maxCount: 3 },
  { name: "transcript", maxCount: 1 },
]);
app.post("/api/projects", (req, res, next) => {
  upload(req, res, (error) => {
    if (error) {
      next(error);
      return;
    }
    void (async () => {
      const files = req.files as Record<string, Express.Multer.File[]>;
      const all = Object.values(files ?? {}).flat();
      try {
        const videos = files?.files ?? [];
        if (!videos.length) throw new Error("Choose one to three video files.");
        const name = z
          .string()
          .trim()
          .min(1)
          .max(120)
          .parse(
            req.body.name || videos[0].originalname.replace(/\.[^.]+$/, ""),
          );
        const project = newProject(name);
        const input: ProcessingInput = {
          manual: req.body.grouping === "manual",
        };
        if (files.transcript?.[0]) {
          const transcriptFile = files.transcript[0];
          if (transcriptFile.size > 20000000)
            throw new Error(
              "Timed transcript JSON must be smaller than 20 MB.",
            );
          let data: unknown;
          try {
            data = JSON.parse(await fs.readFile(transcriptFile.path, "utf8"));
          } catch {
            throw new Error(
              "The optional transcript is not valid JSON. Remove it to transcribe the video automatically with Whisper.",
            );
          }
          const parsed = wordSchema
            .array()
            .min(1)
            .max(200000)
            .safeParse(
              typeof data === "object" && data !== null && "words" in data
                ? data.words
                : undefined,
            );
          if (!parsed.success)
            throw new Error(
              "The optional transcript needs words with text, start and end times. Remove it to let Whisper create the transcript automatically.",
            );
          input.words = parsed.data;
        }
        if (!input.words && !(await speechStatus()).transcription)
          throw new Error(
            "Whisper needs one-time setup on this computer. Run npm run setup:whisper in the project terminal, then upload your video again.",
          );
        await fs.mkdir(projectDir(project.id), { recursive: true });
        for (let i = 0; i < videos.length; i++) {
          const id = (["A", "B", "C"] as CamId[])[i];
          await fs.rename(
            videos[i].path,
            path.join(projectDir(project.id), `source-${id}.mp4`),
          );
          project.cameras.push({
            id,
            name: ["Wide", "Close-up", "Side angle"][i],
            sourcePath: `source-${id}.mp4`,
            proxyPath: `proxy-${id}.mp4`,
            offset: 0,
            duration: 0,
          });
        }
        await writeJson(path.join(projectDir(project.id), "input.json"), input);
        await saveProject(project);
        enqueue(project, input);
        res.status(202).json({ projectId: project.id });
      } finally {
        await Promise.all(all.map((f) => fs.rm(f.path, { force: true })));
      }
    })().catch(next);
  });
});
app.post("/api/projects/:id/resume", async (req, res) => {
  const project = await readProject(String(req.params.id));
  if (project.status !== "error")
    throw new Error("Only an interrupted or failed project can be resumed.");
  const input = await readJson<ProcessingInput>(
    path.join(projectDir(project.id), "input.json"),
  );
  project.status = "uploading";
  project.warnings = project.sample ? project.warnings.slice(0, 1) : [];
  await saveProject(project);
  enqueue(project, input);
  res.status(202).json({ projectId: project.id });
});
app.patch("/api/projects/:id", async (req, res) => {
  const id = String(req.params.id);
  const next = await withProjectWrite(id, async () => {
    const project = await readProject(id);
    if (project.status !== "ready")
      throw new Error("Wait until processing finishes before editing.");
    const edited = applyProjectEdit(project, req.body);
    await saveProject(edited);
    return edited;
  });
  broadcast();
  res.json(next);
});
app.get("/api/projects/:id/media/:cam", async (req, res) => {
  const project = await readProject(String(req.params.id));
  const cam = project.cameras.find((c) => c.id === req.params.cam);
  if (!cam) throw new Error("Camera not found.");
  res.sendFile(path.join(projectDir(project.id), cam.proxyPath));
});
app.get("/api/projects/:id/peaks", async (req, res) => {
  res.json(
    await readJson(path.join(projectDir(String(req.params.id)), "peaks.json")),
  );
});
const exportsById = new Map<string, ExportJob>();
let exportQueue = Promise.resolve();
app.post("/api/projects/:id/exports", async (req, res) => {
  const project = await readProject(String(req.params.id));
  if (project.status !== "ready")
    throw new Error("This project is not ready to export.");
  if (
    [...exportsById.values()].some(
      (j) =>
        j.projectId === project.id &&
        ["queued", "rendering"].includes(j.status),
    )
  )
    throw new Error("This project already has an export running.");
  project.exportSettings = settingsSchema.parse(req.body);
  const job: ExportJob = {
    id: randomUUID(),
    projectId: project.id,
    status: "queued",
    pct: 0,
    detail: "Waiting to render",
    files: [],
    revision: project.revision,
  };
  exportsById.set(job.id, job);
  await writeJson(
    path.join(projectDir(project.id), "exports", job.id, "job.json"),
    job,
  );
  exportQueue = exportQueue.then(() => renderExport(project, job, broadcast));
  res.status(202).json(job);
});
async function exportJob(projectId: string, id: string) {
  if (!/^[\w-]{1,60}$/.test(id)) throw new Error("Invalid export.");
  const job =
    exportsById.get(id) ??
    (await readJson<ExportJob>(
      path.join(projectDir(projectId), "exports", id, "job.json"),
    ));
  if (job.projectId !== projectId) throw new Error("Export not found.");
  if (!exportsById.has(id) && ["queued", "rendering"].includes(job.status)) {
    job.status = "error";
    job.error = "The server stopped during rendering. Start a new export.";
  }
  return job;
}
app.get("/api/projects/:pid/exports/:id", async (req, res) => {
  res.json(await exportJob(String(req.params.pid), String(req.params.id)));
});
app.get("/api/projects/:pid/exports/:id/files/:name", async (req, res) => {
  const job = await exportJob(String(req.params.pid), String(req.params.id));
  const name = String(req.params.name);
  if (job.status !== "done" || !job.files.includes(name))
    throw new Error("That export file is not available.");
  res.download(
    path.join(projectDir(job.projectId), "exports", job.id, name),
    name,
  );
});
app.get("/api/catalyst", async (_req, res) => {
  res.json({
    pipeline: await catalyst.getPipeline(),
    control: await catalyst.getControl(),
    run: catalyst.activeRun(),
    runs: await catalyst.recentRuns(),
    tools: await catalyst.toolsStatus(),
    history: await catalyst.history(),
    proposals: await catalyst.getProposals(),
  });
});
app.get("/api/catalyst/site", async (_req, res) =>
  res.json(await catalyst.scanSite()),
);
app.get("/api/catalyst/files", async (_req, res) =>
  res.json(await catalyst.listFiles()),
);
app.get("/api/catalyst/file", async (req, res) =>
  res.json(await catalyst.readSource(z.string().parse(req.query.path))),
);
app.post("/api/catalyst/proposals", async (req, res) => {
  const p = await catalyst.proposeChange(req.body);
  broadcast();
  res.status(201).json(p);
});
app.post("/api/catalyst/proposals/:id/apply", async (req, res) => {
  const p = await catalyst.applyProposal(String(req.params.id));
  broadcast();
  res.json(p);
});
app.post("/api/catalyst/proposals/:id/discard", async (req, res) => {
  const p = await catalyst.discardProposal(String(req.params.id));
  broadcast();
  res.json(p);
});
app.post("/api/catalyst/proposals/:id/undo", async (req, res) => {
  const p = await catalyst.undoProposal(String(req.params.id));
  broadcast();
  res.json(p);
});
app.patch("/api/catalyst/tasks/:id", async (req, res) => {
  const p = await catalyst.changeTask(String(req.params.id), req.body);
  broadcast();
  res.json(p);
});
app.post("/api/catalyst/tasks", async (req, res) => {
  const p = await catalyst.addTask(req.body);
  broadcast();
  res.json(p);
});
app.post("/api/catalyst/control", async (req, res) => {
  const { paused } = z.object({ paused: z.boolean() }).parse(req.body);
  res.json(await catalyst.setControl(paused));
  broadcast();
});
app.post("/api/catalyst/checks", async (req, res) => {
  const { kind } = z
    .object({ kind: z.enum(["build", "test"]) })
    .parse(req.body);
  res.status(202).json(await catalyst.startCheck(kind, broadcast));
});
app.post("/api/catalyst/checks/cancel", (_req, res) => {
  res.json(catalyst.cancelCheck() ?? {});
  broadcast();
});
app.get("/api/catalyst/roadmap", async (_req, res) => {
  const p = await catalyst.getPipeline();
  const md = [
    "# " + p.name + " roadmap",
    "",
    ...["Now", "Next", "Later"].flatMap((l) => [
      "## " + l,
      "",
      ...p.tasks
        .filter((t) => t.lane === l)
        .flatMap((t) => [
          `- [${t.status === "done" ? "x" : " "}] ${t.title} (${t.status})`,
          `  Done when: ${t.acceptance}`,
        ]),
      "",
    ]),
  ].join("\n");
  res.type("text/markdown").attachment("ROADMAP.md").send(md);
});
app.use("/api", (_req, res) =>
  res.status(404).json({ error: "Endpoint not found." }),
);
app.use(
  express.static(path.join(root, "dist", "web"), {
    index: false,
    dotfiles: "deny",
  }),
);
app.get(["/", "/catalyst"], (_req, res) =>
  res.sendFile(path.join(root, "dist", "web", "index.html")),
);
app.use((error: unknown, _req: Request, res: Response, _next: NextFunction) => {
  const err = error as Error & { status?: number; code?: string };
  const status = err.code === "ENOENT" ? 404 : (err.status ?? 400);
  const message =
    error instanceof ZodError
      ? "Some fields are invalid. Check your input."
      : error instanceof multer.MulterError
        ? "Upload rejected: " + error.message
        : (err.message ?? "The request could not be completed.");
  res.status(status).json({
    error: message
      .replaceAll(root, "[project]")
      .replaceAll(dataRoot, "[storage]"),
  });
});
// Processing is resumable from its saved input and per-step outputs.
for (const id of await fs
  .readdir(path.join(dataRoot, "projects"))
  .catch(() => [])) {
  const project = await readProject(id).catch(() => null);
  if (project && !["ready", "error"].includes(project.status)) {
    project.status = "error";
    project.error =
      "The server stopped during processing. Resume to continue from saved outputs.";
    await saveProject(project);
  }
}
const server = createServer(app);
const port = Number(process.env.PORT ?? 4310);
server.listen(port, "127.0.0.1", () =>
  console.log(
    `Storyfold: http://127.0.0.1:${port}\nCatalyst Solo: http://127.0.0.1:${port}/catalyst`,
  ),
);
process.on("SIGTERM", () => {
  catalyst.cancelCheck();
  server.close();
  for (const res of listeners) res.end();
  process.exit(0);
});
export { app };
