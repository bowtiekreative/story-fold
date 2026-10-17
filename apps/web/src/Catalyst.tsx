import { useEffect, useState, useRef } from "react";
import type {
  Pipeline,
  BuildTask,
  Proposal,
  CheckRun,
} from "../../server/catalyst.ts";
import { api, send } from "./api.ts";
import { Mark, Icon, Badge, Modal, ErrorBanner, useEvents } from "./ui.tsx";
interface Dashboard {
  pipeline: Pipeline;
  control: { paused: boolean };
  run?: CheckRun;
  runs: CheckRun[];
  tools: {
    ffmpeg: boolean;
    ffprobe: boolean;
    transcription: boolean;
    speechProvider: string;
    topicModel: string;
    topicProvider: string;
    topics: boolean;
  };
  history: {
    commits: { id: string; at: string; message: string }[];
    status: string;
  };
  proposals: Proposal[];
}
type Source = { path: string; content: string; hash: string };
type Site = {
  name: string;
  stack: string;
  scannedAt: string;
  notes: string[];
  pages: {
    id: string;
    name: string;
    route: string;
    file: string;
    desc: string;
  }[];
};
const statusLabels = {
  todo: "To do",
  doing: "Doing",
  review: "In review",
  done: "Done",
};
export function Catalyst() {
  const [data, setData] = useState<Dashboard | null>(null),
    [site, setSite] = useState<Site | null>(null),
    [view, setView] = useState("home"),
    [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [notice, setNotice] = useState("");
  const [files, setFiles] = useState<string[]>([]),
    [source, setSource] = useState<Source | null>(null),
    [draft, setDraft] = useState(""),
    [summary, setSummary] = useState(""),
    [review, setReview] = useState<Proposal | null>(null),
    [task, setTask] = useState<BuildTask | null>(null),
    [device, setDevice] = useState("Desktop"),
    [previewKey, setPreviewKey] = useState(0);
  const refreshing = useRef(false),
    fileSearch = useRef<HTMLInputElement>(null);
  const [fileFilter, setFileFilter] = useState("");
  async function refresh() {
    if (refreshing.current) return;
    refreshing.current = true;
    try {
      setData(await api<Dashboard>("/catalyst"));
    } catch (e) {
      setError((e as Error).message);
    } finally {
      refreshing.current = false;
    }
  }
  useEffect(() => {
    document.title = "Catalyst Solo — your Storyfold build";
    void refresh();
    void api<string[]>("/catalyst/files")
      .then(setFiles)
      .catch(() => {});
    void api<Site>("/catalyst/site")
      .then(setSite)
      .catch((e) => setError(e.message));
  }, []);
  useEvents(() => void refresh());
  useEffect(() => {
    const interval = setInterval(() => void refresh(), 4000);
    return () => clearInterval(interval);
  }, []);
  async function action(fn: () => Promise<unknown>, message?: string) {
    setBusy(true);
    setError("");
    try {
      await fn();
      if (message) setNotice(message);
      await refresh();
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  async function openFile(file: string) {
    if (source && draft !== source.content) {
      setError(
        "Your current draft has unsaved edits. Review it or discard the draft before opening another file.",
      );
      return;
    }
    await action(async () => {
      const s = await api<Source>(
        "/catalyst/file?path=" + encodeURIComponent(file),
      );
      setSource(s);
      setDraft(s.content);
      setSummary("");
      setView("code");
    });
  }
  async function propose() {
    if (!source) return;
    await action(async () => {
      const p = await send<Proposal>("/catalyst/proposals", {
        path: source.path,
        after: draft,
        hash: source.hash,
        summary: summary || "Update " + source.path,
      });
      setReview(p);
    });
  }
  async function apply() {
    if (!review) return;
    await action(async () => {
      await send("/catalyst/proposals/" + review.id + "/apply");
      setReview(null);
      if (source?.path === review.path) {
        const s = await api<Source>(
          "/catalyst/file?path=" + encodeURIComponent(review.path),
        );
        setSource(s);
        setDraft(s.content);
      }
      if (review.path === ".catalyst/instructions.md")
        setNotice(
          "Build instructions saved. Send a chat message too if you want to steer the active Codex turn.",
        );
      else
        setNotice(
          "Saved to the real file and committed to Git. You can undo it in History.",
        );
    });
  }
  const nav = [
    ["home", "home", "Overview"],
    ["tasks", "layers", "Tasks & roadmap"],
    ["preview", "film", "Preview"],
    ["pages", "grid", "Pages"],
    ["content", "folder", "Content"],
    ["design", "sun", "Design"],
    ["code", "code", "Code"],
    ["history", "clock", "History"],
    ["activity", "activity", "Activity"],
  ];
  const pending = data?.proposals.filter((p) => p.status === "review") ?? [],
    completed =
      data?.pipeline.tasks.filter((t) => t.status === "done").length ?? 0,
    total = data?.pipeline.tasks.length ?? 0;
  const active = data?.run?.status === "running";
  const check = data?.run ?? data?.runs[0];
  const heading: Record<string, [string, string]> = {
    home: ["YOUR BUILD, IN THE OPEN", "A clear view of what’s taking shape."],
    tasks: ["ONE THING AT A TIME", "Your direction. A shared roadmap."],
    preview: ["SEE IT FOR YOURSELF", "This is your app, running locally."],
    pages: ["A PLACE FOR EVERYTHING", "The views that make up your app."],
    content: ["MAKE IT SOUND LIKE YOU", "Your words, in your hands."],
    design: ["MAKE IT FEEL LIKE YOU", "A few thoughtful details."],
    code: ["UNDER THE SURFACE", "Every file. Every change. Yours."],
    history: ["A WAY BACK, ALWAYS", "Every saved change has a story."],
    activity: [
      "NOTHING BEHIND THE CURTAIN",
      "Here’s what has actually happened.",
    ],
  };
  return (
    <div className="catalyst-shell">
      <aside className="catalyst-sidebar">
        <a className="brand catalyst-brand" href="/catalyst">
          <Mark catalyst />
          <span>
            <strong>Catalyst</strong>
            <small>SOLO</small>
          </span>
        </a>
        <div className="catalyst-project">
          <span className="project-monogram">S</span>
          <span>
            <strong>Storyfold</strong>
            <small>Local development</small>
          </span>
          <span className="live-dot" />
        </div>
        <span className="sidebar-label">YOUR PROJECT</span>
        <nav>
          {nav.map(([key, icon, label]) => (
            <button
              className={view === key ? "active" : ""}
              key={key}
              onClick={() => setView(key)}
            >
              <Icon name={icon} size={18} />
              {label}
              {key === "history" && pending.length > 0 && (
                <span className="nav-count">{pending.length}</span>
              )}
            </button>
          ))}
        </nav>
        <div className="sidebar-bottom">
          <div className="local-indicator">
            <span
              className={
                "live-dot " + (data?.control.paused ? "pending-dot" : "")
              }
            />
            {data?.control.paused
              ? "Local pipeline paused"
              : "Connected to your project"}
          </div>
          <p>
            Plain files. Real Git.
            <br />
            Your work stays yours.
          </p>
          <a
            className="button catalyst-open"
            href="/"
            target="_blank"
            rel="noreferrer"
          >
            Open Storyfold <Icon name="arrow" size={15} />
          </a>
        </div>
      </aside>
      <div className="catalyst-main">
        <header className="catalyst-topbar">
          <span>
            Storyfold <Icon name="chevron" size={13} />{" "}
            <b>{nav.find((n) => n[0] === view)?.[2]}</b>
          </span>
          <div>
            <span className="mono">LOCAL / DRAFT</span>
            <Badge tone="violet">Ponytail · full</Badge>
            <button
              className="button catalyst-secondary small"
              disabled={busy}
              onClick={() =>
                void action(
                  () =>
                    send("/catalyst/control", {
                      paused: !data?.control.paused,
                    }),
                  data?.control.paused
                    ? "Local check runs resumed."
                    : "New local check runs are paused. Use Cancel to stop a run already in progress.",
                )
              }
            >
              <Icon name={data?.control.paused ? "play" : "pause"} size={14} />
              {data?.control.paused ? "Resume pipeline" : "Pause pipeline"}
            </button>
          </div>
        </header>
        <main className="catalyst-content">
          <div className="catalyst-page-title">
            <span className="eyebrow">{heading[view][0]}</span>
            <h1>{heading[view][1]}</h1>
            <p>
              {view === "home"
                ? "See the plan, follow the progress, and change direction whenever you need to."
                : "Changes are local. Source edits are reviewed and committed before they are applied."}
            </p>
          </div>
          <ErrorBanner error={error} />
          {notice && (
            <div className="notice" role="status">
              <Icon name="check" size={16} />
              {notice}
              <button aria-label="Dismiss notice" onClick={() => setNotice("")}>
                ×
              </button>
            </div>
          )}
          {!data ? (
            <div className="catalyst-card">
              <span className="spinner" /> Connecting to the local service…
            </div>
          ) : (
            <>
              {view === "home" && (
                <>
                  <div className="build-overview">
                    <section className="catalyst-card build-current">
                      <div className="card-overline">
                        <span className="live-dot" /> CURRENT MILESTONE{" "}
                        <Badge tone="teal">In progress</Badge>
                      </div>
                      <h2>{data.pipeline.milestone}</h2>
                      <p>{data.pipeline.message}</p>
                      <div className="build-progress-row">
                        <progress max={total} value={completed} />
                        <span className="mono">
                          {completed} / {total}
                        </span>
                      </div>
                      <small>
                        Tasks verified complete ·{" "}
                        {new Date(data.pipeline.updatedAt).toLocaleString()}
                      </small>
                      <div className="card-actions">
                        <button
                          className="button catalyst-primary"
                          onClick={() => setView("tasks")}
                        >
                          See the roadmap <Icon name="arrow" size={15} />
                        </button>
                        <button
                          className="button catalyst-secondary"
                          onClick={() =>
                            void openFile(".catalyst/instructions.md")
                          }
                        >
                          Edit build instructions
                        </button>
                      </div>
                    </section>
                    <section className="catalyst-card your-controls">
                      <span className="eyebrow">YOU’RE IN CONTROL</span>
                      <h2>
                        Take a look.
                        <br />
                        Make it yours.
                      </h2>
                      <p>
                        Preview the app, change the copy or adjust the source.
                        Every applied edit has a way back.
                      </p>
                      <button onClick={() => setView("preview")}>
                        <Icon name="play" size={17} />
                        <span>Open the live preview</span>
                        <Icon name="arrow" size={16} />
                      </button>
                      <button
                        onClick={() => void openFile("apps/web/src/copy.json")}
                      >
                        <Icon name="folder" size={17} />
                        <span>Edit the app’s words</span>
                        <Icon name="arrow" size={16} />
                      </button>
                      <button onClick={() => setView("history")}>
                        <Icon name="clock" size={17} />
                        <span>Review changes & undo</span>
                        <Icon name="arrow" size={16} />
                      </button>
                    </section>
                  </div>
                  <div className="catalyst-stats">
                    <div>
                      <span className="stat-icon">
                        <Icon name="grid" />
                      </span>
                      <strong>{site?.pages.length ?? 2}</strong>
                      <span>app views</span>
                    </div>
                    <div>
                      <span className="stat-icon">
                        <Icon name="code" />
                      </span>
                      <strong>{files.length}</strong>
                      <span>visible source files</span>
                    </div>
                    <div>
                      <span className="stat-icon">
                        <Icon name="clock" />
                      </span>
                      <strong>{data.history.commits.length}</strong>
                      <span>recent Git commits</span>
                    </div>
                    <div>
                      <span className="stat-icon">
                        <Icon name="layers" />
                      </span>
                      <strong>{pending.length}</strong>
                      <span>waiting for review</span>
                    </div>
                  </div>
                  <div className="catalyst-two-columns">
                    <section className="catalyst-card">
                      <div className="card-title">
                        <h2>On the workbench</h2>
                        <button
                          className="text-button"
                          onClick={() => setView("tasks")}
                        >
                          All tasks <Icon name="arrow" size={14} />
                        </button>
                      </div>
                      {data.pipeline.tasks
                        .filter((t) => t.lane === "Now")
                        .map((t) => (
                          <button
                            className="workbench-task"
                            key={t.id}
                            onClick={() => setTask({ ...t })}
                          >
                            <span className={"task-indicator " + t.status}>
                              {t.status === "done" ? (
                                <Icon name="check" size={13} />
                              ) : null}
                            </span>
                            <span>
                              <strong>{t.title}</strong>
                              <small>{t.acceptance}</small>
                            </span>
                            <Badge
                              tone={
                                t.status === "done"
                                  ? "teal"
                                  : t.status === "doing"
                                    ? "violet"
                                    : "neutral"
                              }
                            >
                              {statusLabels[t.status]}
                            </Badge>
                          </button>
                        ))}
                    </section>
                    <section className="catalyst-card">
                      <div className="card-title">
                        <h2>Ready for a check?</h2>
                        <Icon name="activity" size={20} />
                      </div>
                      <p>
                        Run the actual project build and tests. The output below
                        comes from this computer.
                      </p>
                      <div className="check-buttons">
                        <button
                          className="button catalyst-secondary"
                          disabled={busy || active || data.control.paused}
                          onClick={() =>
                            void action(() =>
                              send("/catalyst/checks", { kind: "build" }),
                            )
                          }
                        >
                          <Icon name="code" size={16} /> Build app
                        </button>
                        <button
                          className="button catalyst-secondary"
                          disabled={busy || active || data.control.paused}
                          onClick={() =>
                            void action(() =>
                              send("/catalyst/checks", { kind: "test" }),
                            )
                          }
                        >
                          <Icon name="check" size={16} /> Run tests
                        </button>
                        {active && (
                          <button
                            className="button danger"
                            onClick={() =>
                              void action(() => send("/catalyst/checks/cancel"))
                            }
                          >
                            Cancel run
                          </button>
                        )}
                      </div>
                      {check ? (
                        <>
                          <div className="check-status">
                            <span>
                              {check.kind === "build"
                                ? "Production build"
                                : "Unit & integration tests"}
                            </span>
                            <Badge
                              tone={
                                check.status === "passed"
                                  ? "teal"
                                  : check.status === "failed"
                                    ? "coral"
                                    : "violet"
                              }
                            >
                              {check.status}
                            </Badge>
                          </div>
                          <pre className="check-output" aria-live="polite">
                            {check.output}
                          </pre>
                        </>
                      ) : (
                        <div className="check-empty">
                          <Icon name="activity" />
                          <span>No check runs recorded yet.</span>
                        </div>
                      )}
                      <p className="small muted">
                        Pause and Cancel control these local runs. To steer or
                        stop Codex, use chat.
                      </p>
                    </section>
                  </div>
                  <section className="catalyst-card environment-card">
                    <h2>A quick health check</h2>
                    <div>
                      <span>
                        Media tools{" "}
                        <Badge
                          tone={
                            data.tools.ffmpeg && data.tools.ffprobe
                              ? "teal"
                              : "coral"
                          }
                        >
                          {data.tools.ffmpeg && data.tools.ffprobe
                            ? "ffmpeg + ffprobe ready"
                            : "Needs setup"}
                        </Badge>
                      </span>
                      <span>
                        Transcription{" "}
                        <Badge
                          tone={data.tools.transcription ? "teal" : "amber"}
                        >
                          {data.tools.transcription
                            ? `${data.tools.speechProvider} configured`
                            : "Whisper setup needed"}
                        </Badge>
                      </span>
                      <span>
                        Topic folders{" "}
                        <Badge tone={data.tools.topics ? "teal" : "amber"}>
                          {data.tools.topics
                            ? `${data.tools.topicProvider} configured`
                            : "Manual topics"}
                        </Badge>
                      </span>
                      <span>
                        Publishing <Badge tone="neutral">Not configured</Badge>
                      </span>
                    </div>
                    <small>
                      Provider readiness does not verify credits or
                      transcription quality. No secrets are displayed.
                    </small>
                  </section>
                </>
              )}
              {view === "tasks" && (
                <>
                  <div className="view-actions">
                    <a
                      className="button catalyst-secondary"
                      href="/api/catalyst/roadmap"
                    >
                      <Icon name="download" size={16} /> Export roadmap
                    </a>
                    <button
                      className="button catalyst-primary"
                      onClick={() =>
                        setTask({
                          id: "new",
                          title: "",
                          acceptance: "",
                          lane: "Next",
                          status: "todo",
                          evidence: [],
                        })
                      }
                    >
                      <Icon name="plus" size={16} /> Add a task
                    </button>
                  </div>
                  <div className="roadmap-lanes">
                    {(["Now", "Next", "Later"] as const).map((lane) => (
                      <section className="roadmap-lane" key={lane}>
                        <div className="lane-heading">
                          <h2>{lane}</h2>
                          <span>
                            {
                              data.pipeline.tasks.filter((t) => t.lane === lane)
                                .length
                            }
                          </span>
                        </div>
                        {data.pipeline.tasks
                          .filter((t) => t.lane === lane)
                          .map((t) => (
                            <button
                              className="task-card"
                              key={t.id}
                              onClick={() => setTask({ ...t })}
                            >
                              <Badge
                                tone={
                                  t.status === "done"
                                    ? "teal"
                                    : t.status === "doing"
                                      ? "violet"
                                      : "neutral"
                                }
                              >
                                {statusLabels[t.status]}
                              </Badge>
                              <h3>{t.title}</h3>
                              <p>{t.acceptance}</p>
                              {t.evidence.length > 0 && (
                                <small>
                                  <Icon name="check" size={13} />
                                  {t.evidence.length} evidence{" "}
                                  {t.evidence.length === 1 ? "file" : "files"}
                                </small>
                              )}
                            </button>
                          ))}
                      </section>
                    ))}
                  </div>
                  <p className="note">
                    You can edit the direction and acceptance criteria. Marking
                    a task Done records your decision; it does not manufacture
                    test evidence.
                  </p>
                </>
              )}
              {view === "pages" && (
                <>
                  <div className="view-actions">
                    <span className="muted">
                      {site?.stack} · scanned{" "}
                      {site
                        ? new Date(site.scannedAt).toLocaleTimeString()
                        : ""}
                    </span>
                    <button
                      className="button catalyst-secondary"
                      onClick={() =>
                        void action(async () =>
                          setSite(await api<Site>("/catalyst/site")),
                        )
                      }
                    >
                      Scan again
                    </button>
                  </div>
                  <div className="pages-grid">
                    {site?.pages.map((p) => (
                      <section className="catalyst-card page-card" key={p.id}>
                        <span className="page-preview-icon">
                          <Icon
                            name={p.id === "editor" ? "film" : "layers"}
                            size={36}
                          />
                        </span>
                        <Badge tone="neutral">Custom app view</Badge>
                        <h2>{p.name}</h2>
                        <span className="mono muted">{p.route}</span>
                        <p>{p.desc}</p>
                        <div className="card-actions">
                          <a
                            className="button catalyst-primary"
                            href={p.route}
                            target="_blank"
                            rel="noreferrer"
                          >
                            Open view <Icon name="arrow" size={15} />
                          </a>
                          <button
                            className="button catalyst-secondary"
                            onClick={() => void openFile(p.file)}
                          >
                            Edit source
                          </button>
                        </div>
                      </section>
                    ))}
                  </div>
                  <p className="note">{site?.notes.join(" ")}</p>
                </>
              )}
              {(view === "content" || view === "design") && (
                <section className="catalyst-card content-intro">
                  <span className="large-view-icon">
                    <Icon
                      name={view === "content" ? "folder" : "sun"}
                      size={32}
                    />
                  </span>
                  <h2>
                    {view === "content"
                      ? "Start with the words people see."
                      : "Keep the feel. Change the details."}
                  </h2>
                  <p>
                    {view === "content"
                      ? "The welcome screen’s headline, introduction and upload copy live in one small file. Edit the text, review the difference, then apply it."
                      : "Storyfold uses the supplied CaroSlides colors, type and spacing. Edit the stylesheet to adjust the app; the preview updates after you apply a reviewed change."}
                  </p>
                  <button
                    className="button catalyst-primary"
                    onClick={() =>
                      void openFile(
                        view === "content"
                          ? "apps/web/src/copy.json"
                          : "apps/web/src/styles.css",
                      )
                    }
                  >
                    Edit {view === "content" ? "app copy" : "the stylesheet"}{" "}
                    <Icon name="arrow" size={16} />
                  </button>
                  <button
                    className="button catalyst-secondary"
                    onClick={() => setView("preview")}
                  >
                    See the preview
                  </button>
                </section>
              )}
              {view === "code" && (
                <div className="source-workspace">
                  <aside className="source-files">
                    <label>
                      <Icon name="search" size={15} />
                      <input
                        ref={fileSearch}
                        aria-label="Filter source files"
                        placeholder="Find a file…"
                        value={fileFilter}
                        onChange={(e) => setFileFilter(e.target.value)}
                      />
                    </label>
                    {files
                      .filter((f) =>
                        f.toLowerCase().includes(fileFilter.toLowerCase()),
                      )
                      .map((f) => (
                        <button
                          className={source?.path === f ? "active" : ""}
                          key={f}
                          onClick={() => void openFile(f)}
                          title={f}
                        >
                          <Icon name="code" size={14} />
                          <span>{f}</span>
                        </button>
                      ))}
                  </aside>
                  <section className="source-editor">
                    {source ? (
                      <>
                        <div className="source-editor-heading">
                          <span className="mono">{source.path}</span>
                          <Badge
                            tone={
                              draft === source.content ? "neutral" : "amber"
                            }
                          >
                            {draft === source.content
                              ? "Saved source"
                              : "Draft edit"}
                          </Badge>
                        </div>
                        <textarea
                          spellCheck={false}
                          aria-label="Source file editor"
                          value={draft}
                          onChange={(e) => setDraft(e.target.value)}
                        />
                        <div className="source-edit-actions">
                          <label>
                            What does this change do?
                            <input
                              placeholder="A short summary for Git history"
                              value={summary}
                              maxLength={200}
                              onChange={(e) => setSummary(e.target.value)}
                            />
                          </label>
                          <div>
                            <button
                              className="button catalyst-secondary"
                              disabled={busy || draft === source.content}
                              onClick={() => {
                                setDraft(source.content);
                                setSummary("");
                              }}
                            >
                              Discard draft
                            </button>
                            <button
                              className="button catalyst-primary"
                              disabled={busy || draft === source.content}
                              onClick={() => void propose()}
                            >
                              Review change <Icon name="arrow" size={16} />
                            </button>
                          </div>
                        </div>
                      </>
                    ) : (
                      <div className="source-empty">
                        <Icon name="code" size={32} />
                        <h2>See exactly what’s being built.</h2>
                        <p>
                          Choose a source file. Edits stay as drafts until you
                          review and apply them.
                        </p>
                        <button
                          className="button catalyst-primary"
                          onClick={() =>
                            void openFile(".catalyst/instructions.md")
                          }
                        >
                          Open your build instructions
                        </button>
                      </div>
                    )}
                  </section>
                </div>
              )}
              {view === "preview" && (
                <div className="preview-workspace">
                  <div className="preview-toolbar">
                    <div className="segmented">
                      {["Desktop", "Tablet", "Phone"].map((d) => (
                        <button
                          key={d}
                          className={device === d ? "active" : ""}
                          onClick={() => setDevice(d)}
                        >
                          {d}
                        </button>
                      ))}
                    </div>
                    <span className="mono muted">DRAFT · LOCAL APP</span>
                    <button
                      className="button catalyst-secondary small"
                      onClick={() => setPreviewKey((k) => k + 1)}
                    >
                      Refresh
                    </button>
                    <a
                      className="button catalyst-primary small"
                      href="/"
                      target="_blank"
                      rel="noreferrer"
                    >
                      Open in a tab <Icon name="arrow" size={14} />
                    </a>
                  </div>
                  <div
                    className="preview-device"
                    style={{
                      maxWidth:
                        device === "Phone"
                          ? 390
                          : device === "Tablet"
                            ? 820
                            : "100%",
                    }}
                  >
                    <iframe
                      key={previewKey}
                      title="Storyfold live preview"
                      src="/"
                    />
                  </div>
                </div>
              )}
              {view === "history" && (
                <>
                  <section className="catalyst-card">
                    <div className="card-title">
                      <h2>Waiting for your review</h2>
                      <Badge tone="violet">{pending.length}</Badge>
                    </div>
                    {pending.length ? (
                      pending.map((p) => (
                        <div className="review-row" key={p.id}>
                          <span className="review-dot">
                            <Icon name="code" size={17} />
                          </span>
                          <div>
                            <strong>{p.summary}</strong>
                            <small>
                              {p.path} ·{" "}
                              {new Date(p.createdAt).toLocaleString()}
                            </small>
                          </div>
                          <button
                            className="button catalyst-primary small"
                            onClick={() => setReview(p)}
                          >
                            Review
                          </button>
                        </div>
                      ))
                    ) : (
                      <p className="muted">
                        No pending changes. Edit a file to create a review.
                      </p>
                    )}
                  </section>
                  <section className="catalyst-card">
                    <h2>Changes you applied</h2>
                    {data.proposals.filter((p) => p.status === "applied")
                      .length ? (
                      data.proposals
                        .filter((p) => p.status === "applied")
                        .map((p) => (
                          <div className="review-row" key={p.id}>
                            <span className="review-dot teal">
                              <Icon name="check" size={16} />
                            </span>
                            <div>
                              <strong>{p.summary}</strong>
                              <small>
                                <span className="mono">
                                  {p.commit?.slice(0, 7)}
                                </span>{" "}
                                · {p.path}
                              </small>
                            </div>
                            <button
                              className="button catalyst-secondary small"
                              disabled={busy}
                              onClick={() =>
                                void action(async () =>
                                  setReview(
                                    await send<Proposal>(
                                      "/catalyst/proposals/" + p.id + "/undo",
                                    ),
                                  ),
                                )
                              }
                            >
                              Review undo <Icon name="undo" size={14} />
                            </button>
                          </div>
                        ))
                    ) : (
                      <p className="muted">
                        Your reviewed edits will appear here with an Undo
                        action.
                      </p>
                    )}
                  </section>
                  <section className="catalyst-card">
                    <div className="card-title">
                      <h2>Real Git history</h2>
                      <Badge tone="neutral">Local commits</Badge>
                    </div>
                    {data.history.commits.map((c) => (
                      <div className="commit-row" key={c.id}>
                        <span className="commit-node" />
                        <code>{c.id}</code>
                        <strong>{c.message}</strong>
                        <time>{new Date(c.at).toLocaleString()}</time>
                      </div>
                    ))}
                  </section>
                  <details className="catalyst-card">
                    <summary>Working folder changes</summary>
                    <pre className="check-output">
                      {data.history.status || "Working tree is clean."}
                    </pre>
                    <p className="small muted">
                      Reference prototypes and unrelated files are kept as they
                      were. Catalyst commits only the file you reviewed.
                    </p>
                  </details>
                </>
              )}
              {view === "activity" && (
                <section className="catalyst-card">
                  <h2>The activity log</h2>
                  {data.pipeline.message && (
                    <div className="activity-item">
                      <span className="review-dot">
                        <Icon name="activity" />
                      </span>
                      <div>
                        <strong>{data.pipeline.message}</strong>
                        <small>
                          Build coordinator ·{" "}
                          {new Date(data.pipeline.updatedAt).toLocaleString()}
                        </small>
                      </div>
                    </div>
                  )}
                  {data.proposals.map((p) => (
                    <div className="activity-item" key={p.id}>
                      <span className="review-dot">
                        <Icon name="code" size={17} />
                      </span>
                      <div>
                        <strong>{p.summary}</strong>
                        <small>
                          {p.path} · {p.status}
                          {p.commit ? " · " + p.commit.slice(0, 7) : ""} ·{" "}
                          {new Date(p.createdAt).toLocaleString()}
                        </small>
                      </div>
                    </div>
                  ))}
                  {data.runs.map((r) => (
                    <div className="activity-item" key={r.id}>
                      <span className="review-dot">
                        <Icon name="check" size={17} />
                      </span>
                      <div>
                        <strong>
                          {r.kind === "build" ? "Build" : "Tests"} · {r.status}
                        </strong>
                        <small>
                          {new Date(r.startedAt).toLocaleString()} · exit{" "}
                          {r.exitCode ?? "pending"}
                        </small>
                      </div>
                    </div>
                  ))}
                  <p className="small muted">
                    Shows recorded local build events and reviewed edits. Git
                    History also includes changes made directly by the coding
                    agent.
                  </p>
                </section>
              )}
            </>
          )}
        </main>
        <footer className="catalyst-footer">
          <span>
            <span className="live-dot" /> Storyfold · a project you can
            understand and own.
          </span>
          <span>Draft locally. Review every change.</span>
        </footer>
      </div>
      {review && (
        <Modal
          title="See it before you save it."
          wide
          onClose={() => setReview(null)}
        >
          <div className="review-summary">
            <Badge tone="violet">
              {review.undoOf ? "Undo proposal" : "Source change"}
            </Badge>
            <h3>{review.summary}</h3>
            <code>{review.path}</code>
            <p>
              This writes to the real file and creates one Git commit. It stays
              local until you choose to publish it.
            </p>
          </div>
          <div className="diff-grid">
            <div>
              <h4>Before</h4>
              <pre>{review.before}</pre>
            </div>
            <div>
              <h4>After</h4>
              <pre>{review.after}</pre>
            </div>
          </div>
          <ErrorBanner error={error} />
          <div className="modal-actions">
            <button
              className="button secondary"
              disabled={busy}
              onClick={() => {
                setSource({
                  path: review.path,
                  content: review.before,
                  hash: review.hash,
                });
                setDraft(review.after);
                setSummary(review.summary);
                setView("code");
                setReview(null);
              }}
            >
              Edit this draft
            </button>
            <button
              className="button secondary"
              disabled={busy}
              onClick={() =>
                void action(async () => {
                  await send("/catalyst/proposals/" + review.id + "/discard");
                  setReview(null);
                })
              }
            >
              Discard proposal
            </button>
            <button
              className="button primary"
              disabled={busy}
              onClick={() => void apply()}
            >
              {busy ? "Applying…" : "Apply & commit"}
              <Icon name="check" size={16} />
            </button>
          </div>
        </Modal>
      )}
      {task && (
        <Modal
          title={
            task.id === "new"
              ? "Give the build a direction."
              : "What does done look like?"
          }
          onClose={() => setTask(null)}
        >
          <label>
            Task title
            <input
              autoFocus
              maxLength={160}
              value={task.title}
              onChange={(e) => setTask({ ...task, title: e.target.value })}
            />
          </label>
          <label>
            Done when
            <textarea
              rows={4}
              maxLength={2000}
              value={task.acceptance}
              onChange={(e) => setTask({ ...task, acceptance: e.target.value })}
            />
          </label>
          <div className="two-fields">
            <label>
              Roadmap lane
              <select
                value={task.lane}
                onChange={(e) =>
                  setTask({
                    ...task,
                    lane: e.target.value as BuildTask["lane"],
                  })
                }
              >
                {["Now", "Next", "Later"].map((l) => (
                  <option key={l}>{l}</option>
                ))}
              </select>
            </label>
            <label>
              Status
              <select
                value={task.status}
                onChange={(e) =>
                  setTask({
                    ...task,
                    status: e.target.value as BuildTask["status"],
                  })
                }
              >
                {Object.entries(statusLabels).map(([key, value]) => (
                  <option value={key} key={key}>
                    {value}
                  </option>
                ))}
              </select>
            </label>
          </div>
          {task.evidence.length > 0 && (
            <p className="note small">
              Recorded evidence: {task.evidence.join(", ")}
            </p>
          )}
          <button
            className="button primary"
            disabled={busy || !task.title.trim() || !task.acceptance.trim()}
            onClick={() =>
              void action(async () => {
                await send(
                  task.id === "new"
                    ? "/catalyst/tasks"
                    : "/catalyst/tasks/" + task.id,
                  task,
                  task.id === "new" ? "POST" : "PATCH",
                );
                setTask(null);
              }, "Roadmap saved. These are your build directions.")
            }
          >
            Save task <Icon name="check" size={16} />
          </button>
        </Modal>
      )}
    </div>
  );
}
