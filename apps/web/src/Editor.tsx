import {
  useEffect,
  useState,
  useRef,
  useMemo,
  type DragEvent,
  type CSSProperties,
} from "react";
import {
  sequence,
  fmt,
  type Project,
  type Sentence,
  type TimelineItem,
  type Accent,
} from "../../../packages/timeline/index.ts";
import type { ExportJob } from "../../server/media.ts";
import { api, send } from "./api.ts";
import { Mark, Icon, Badge, Modal, ErrorBanner, useEvents } from "./ui.tsx";
import { Player } from "./Player.tsx";
const accentStyle = (accent: string): CSSProperties =>
  ({
    "--topic-solid": `var(--${accent}-600)`,
    "--topic-tint": `var(--${accent}-100)`,
    "--topic-wash": `var(--${accent}-50)`,
  }) as CSSProperties;
type DragItem = TimelineItem | { kind: "move"; index: number };
export function Editor({
  initial,
  onHome,
}: {
  initial: Project;
  onHome: () => void;
}) {
  const [project, setProject] = useState(initial),
    [selectedId, setSelectedId] = useState(initial.sentences[0]?.id ?? ""),
    [folder, setFolder] = useState("all"),
    [search, setSearch] = useState("");
  const [time, setTime] = useState(0),
    [playing, setPlaying] = useState(false),
    [mode, setMode] = useState<"story" | "sentence">("sentence"),
    [zoom, setZoom] = useState(14),
    [drop, setDrop] = useState<number | null>(null);
  const [editVersion, setEditVersion] = useState(0),
    [saveState, setSaveState] = useState("Saved on this computer"),
    [error, setError] = useState(""),
    [peaks, setPeaks] = useState<{ rate: number; peaks: number[] }>({
      rate: 20,
      peaks: [],
    });
  const [exportOpen, setExportOpen] = useState(false),
    [exportJob, setExportJob] = useState<ExportJob | null>(null),
    [exportBusy, setExportBusy] = useState(false),
    [topicEditor, setTopicEditor] = useState<string | null>(null),
    [topicName, setTopicName] = useState(""),
    [mergeTarget, setMergeTarget] = useState(""),
    [syncOpen, setSyncOpen] = useState(false);
  const undoStack = useRef<Project[]>([]),
    redoStack = useRef<Project[]>([]),
    latest = useRef(project),
    revision = useRef(project.revision),
    dirty = useRef(0),
    saved = useRef(0),
    saving = useRef<Promise<void> | null>(null);
  const sq = useMemo(() => sequence(project), [project]),
    selected = project.sentences.find((s) => s.id === selectedId),
    topic = project.topics.find((t) => t.id === selected?.topicId);
  latest.current = project;
  useEffect(() => {
    void api<{ rate: number; peaks: number[] }>(`/projects/${project.id}/peaks`)
      .then(setPeaks)
      .catch(() => {});
  }, [project.id]);
  function update(fn: (p: Project) => Project, record = true) {
    setPlaying(false);
    if (record) {
      undoStack.current.push(latest.current);
      if (undoStack.current.length > 100) undoStack.current.shift();
      redoStack.current = [];
    }
    const next = fn(latest.current);
    latest.current = next;
    setProject(next);
    dirty.current++;
    setEditVersion((v) => v + 1);
    setSaveState("Unsaved changes");
  }
  function editSentence(patch: Partial<Sentence>) {
    if (selected)
      update((p) => ({
        ...p,
        sentences: p.sentences.map((s) =>
          s.id === selected.id ? { ...s, ...patch } : s,
        ),
      }));
  }
  function undo() {
    const p = undoStack.current.pop();
    if (p) {
      redoStack.current.push(latest.current);
      update(() => ({ ...p, revision: revision.current }), false);
    }
  }
  function redo() {
    const p = redoStack.current.pop();
    if (p) {
      undoStack.current.push(latest.current);
      update(() => ({ ...p, revision: revision.current }), false);
    }
  }
  async function save() {
    if (saving.current) {
      await saving.current;
      if (saved.current < dirty.current) return save();
      return;
    }
    if (saved.current === dirty.current) return;
    const count = dirty.current,
      snapshot = latest.current;
    setSaveState("Saving…");
    const work = (async () => {
      try {
        const next = await send<Project>(
          `/projects/${project.id}`,
          {
            ...snapshot,
            revision: revision.current,
            offsets: Object.fromEntries(
              snapshot.cameras.map((c) => [c.id, c.offset]),
            ),
          },
          "PATCH",
        );
        revision.current = next.revision;
        saved.current = count;
        latest.current = { ...latest.current, revision: next.revision };
        setProject(latest.current);
        setSaveState(
          saved.current === dirty.current
            ? "Saved on this computer"
            : "Unsaved changes",
        );
        setError("");
      } catch (e) {
        setSaveState("Save needs attention");
        setError((e as Error).message);
        throw e;
      }
    })();
    saving.current = work;
    try {
      await work;
    } finally {
      saving.current = null;
    }
  }
  useEffect(() => {
    if (!editVersion) return;
    const timer = setTimeout(() => {
      void save().catch(() => {});
    }, 1000);
    return () => clearTimeout(timer);
  }, [editVersion]);
  useEffect(() => {
    const before = (e: BeforeUnloadEvent) => {
      if (dirty.current > saved.current) {
        e.preventDefault();
      }
    };
    window.addEventListener("beforeunload", before);
    return () => window.removeEventListener("beforeunload", before);
  }, []);
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const el = e.target as HTMLElement;
      if (
        ["INPUT", "TEXTAREA", "SELECT"].includes(el.tagName) ||
        el.isContentEditable ||
        document.querySelector("dialog[open]")
      )
        return;
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "z") {
        e.preventDefault();
        if (e.shiftKey) redo();
        else undo();
      }
      if (e.code === "Space") {
        e.preventDefault();
        togglePlay();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [playing, mode, selectedId, sq.total]);
  useEvents(() => {
    if (exportJob && ["queued", "rendering"].includes(exportJob.status))
      void api<ExportJob>(`/projects/${project.id}/exports/${exportJob.id}`)
        .then(setExportJob)
        .catch((e) => setError(e.message));
  });
  useEffect(() => {
    if (!exportJob || !["queued", "rendering"].includes(exportJob.status))
      return;
    const timer = setInterval(
      () =>
        void api<ExportJob>(`/projects/${project.id}/exports/${exportJob.id}`)
          .then(setExportJob)
          .catch(() => {}),
      2000,
    );
    return () => clearInterval(timer);
  }, [exportJob?.id, exportJob?.status]);
  const addItem = (
    item: TimelineItem,
    index = latest.current.timeline.length,
  ) => {
    update((p) => ({
      ...p,
      timeline: [
        ...p.timeline.slice(0, index),
        item,
        ...p.timeline.slice(index),
      ],
    }));
  };
  function select(s: Sentence, play = false) {
    setSelectedId(s.id);
    setMode("sentence");
    setTime(0);
    setPlaying(play);
  }
  function togglePlay() {
    const length =
      mode === "story" ? sq.total : selected ? selected.out - selected.in : 0;
    if (!length) return;
    if (time >= length - 0.03) setTime(0);
    setPlaying((v) => !v);
  }
  function playStory() {
    setMode("story");
    if (mode !== "story" || time >= sq.total - 0.03) setTime(0);
    setPlaying(mode === "story" ? !playing : true);
  }
  function dragStart(e: DragEvent, item: DragItem) {
    e.dataTransfer.setData("application/storyfold", JSON.stringify(item));
    e.dataTransfer.effectAllowed = "copyMove";
  }
  function insertion(e: DragEvent) {
    const box = e.currentTarget.getBoundingClientRect(),
      t = Math.max(0, (e.clientX - box.left) / zoom);
    const i = sq.spans.findIndex((s) => t < (s.start + s.end) / 2);
    return i < 0 ? project.timeline.length : i;
  }
  function dropItem(e: DragEvent) {
    e.preventDefault();
    try {
      const item = JSON.parse(
        e.dataTransfer.getData("application/storyfold"),
      ) as DragItem;
      const at = drop ?? insertion(e);
      if (item.kind === "move") {
        update((p) => {
          const list = [...p.timeline];
          const [moved] = list.splice(item.index, 1);
          list.splice(item.index < at ? at - 1 : at, 0, moved);
          return { ...p, timeline: list };
        });
      } else if (item.kind === "group" || item.kind === "clip")
        addItem(item, at);
    } catch {
      /* Ignore unrelated external drags. */
    }
    setDrop(null);
  }
  function autoCams() {
    update((p) => {
      const seen = new Set<string>();
      let k = 0;
      return {
        ...p,
        sentences: p.sentences.map((s) => {
          const cam = !seen.has(s.topicId)
            ? "A"
            : p.cameras[++k % p.cameras.length].id;
          seen.add(s.topicId);
          return { ...s, cam };
        }),
      };
    });
  }
  const filtered = project.sentences.filter(
    (s) =>
      (folder === "all" || s.topicId === folder) &&
      s.text.toLowerCase().includes(search.toLowerCase()),
  );
  const folderTopic = project.topics.find((t) => t.id === folder),
    duration =
      mode === "story" ? sq.total : selected ? selected.out - selected.in : 0;
  async function startExport() {
    setExportBusy(true);
    setError("");
    try {
      await save();
      const job = await send<ExportJob>(
        `/projects/${project.id}/exports`,
        latest.current.exportSettings,
      );
      setExportJob(job);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setExportBusy(false);
    }
  }
  function saveTopic() {
    const name = topicName.trim();
    if (!name) return;
    update((p) =>
      topicEditor === "new"
        ? {
            ...p,
            topics: [
              ...p.topics,
              {
                id: "t-" + crypto.randomUUID(),
                name,
                accent: (
                  [
                    "violet",
                    "coral",
                    "teal",
                    "amber",
                    "sky",
                    "lime",
                  ] as Accent[]
                )[p.topics.length % 6],
              },
            ],
          }
        : {
            ...p,
            topics: p.topics.map((t) =>
              t.id === topicEditor ? { ...t, name } : t,
            ),
          },
    );
    setTopicEditor(null);
  }
  function mergeTopic() {
    if (!mergeTarget || !topicEditor || topicEditor === "new") return;
    update((p) => ({
      ...p,
      topics: p.topics.filter((t) => t.id !== topicEditor),
      sentences: p.sentences.map((s) =>
        s.topicId === topicEditor ? { ...s, topicId: mergeTarget } : s,
      ),
      timeline: p.timeline
        .map((it) =>
          it.kind === "group" && it.topicId === topicEditor
            ? { ...it, topicId: mergeTarget }
            : it,
        )
        .filter(
          (it, i, arr) =>
            it.kind !== "group" ||
            it.topicId !== mergeTarget ||
            arr.findIndex(
              (x) => x.kind === "group" && x.topicId === mergeTarget,
            ) === i,
        ),
    }));
    if (folder === topicEditor) setFolder(mergeTarget);
    setTopicEditor(null);
  }
  const min = selected ? Math.max(0, selected.start - 1.5) : 0,
    max = selected
      ? Math.min(project.cameras[0].duration, selected.end + 1.5)
      : 0;
  const wave = Array.from(
    { length: 100 },
    (_, i) =>
      peaks.peaks[Math.floor((min + ((max - min) * i) / 100) * peaks.rate)] ??
      0,
  );
  return (
    <div className="editor-shell">
      <header className="app-header editor-header">
        <button
          className="brand brand-button"
          onClick={() => {
            void save()
              .then(onHome)
              .catch(() => {});
          }}
        >
          <Mark />
          <strong>Storyfold</strong>
        </button>
        <span className="brand-divider" />
        <input
          className="project-title"
          aria-label="Project name"
          value={project.name}
          maxLength={120}
          onChange={(e) => {
            if (e.target.value.trim())
              update((p) => ({ ...p, name: e.target.value }));
          }}
        />
        <span className="save-state">
          <span
            className={
              "live-dot " + (saveState.startsWith("Saved") ? "" : "pending-dot")
            }
          />
          {saveState}
        </span>
        <div className="header-actions">
          <a
            className="button quiet"
            href="/catalyst"
            target="_blank"
            rel="noreferrer"
          >
            <Icon name="layers" size={16} /> Your build
          </a>
          <button
            className="button secondary"
            onClick={playStory}
            disabled={!sq.total}
          >
            <Icon
              name={playing && mode === "story" ? "pause" : "play"}
              size={16}
            />
            {playing && mode === "story" ? "Pause story" : "Play story"}
          </button>
          <button
            className="button primary"
            onClick={() => setExportOpen(true)}
            disabled={!sq.total}
          >
            Export <Icon name="arrow" size={16} />
          </button>
        </div>
      </header>
      {error && (
        <div className="editor-error">
          <ErrorBanner error={error} />
          <button
            className="button quiet"
            onClick={() => void save().catch(() => {})}
          >
            Retry save
          </button>
        </div>
      )}
      <div className="workspace-bar">
        <span>
          <span className="live-dot" /> YOUR STORY, YOUR WAY
        </span>
        <div>
          <span>{project.sentences.length} sentences</span>
          <span>·</span>
          <span>{project.topics.length} topics</span>
          <span>·</span>
          <span>
            {project.cameras.filter((c) => !c.virtual).length === 1
              ? "1 camera + 2 reframes"
              : `${project.cameras.length} cameras`}
          </span>
          {project.sample && <Badge tone="amber">Synthetic sample</Badge>}
        </div>
      </div>
      <div className="editor-main">
        <aside className="topics-panel">
          <div className="panel-heading">
            <span className="eyebrow">YOUR TOPICS</span>
            <button
              className="icon-btn"
              aria-label="Create a topic folder"
              onClick={() => {
                setTopicName("");
                setTopicEditor("new");
              }}
            >
              <Icon name="plus" size={17} />
            </button>
          </div>
          <p className="panel-hint">A place for every part of the story.</p>
          <button
            className={"all-sentences " + (folder === "all" ? "active" : "")}
            onClick={() => setFolder("all")}
          >
            <Icon name="layers" size={18} />
            <span>All sentences</span>
            <span>{project.sentences.length}</span>
          </button>
          <div className="topic-list">
            {project.topics.map((t) => {
              const list = project.sentences.filter((s) => s.topicId === t.id);
              return (
                <div
                  className={
                    "topic-folder " + (folder === t.id ? "active" : "")
                  }
                  style={accentStyle(t.accent)}
                  key={t.id}
                  draggable
                  onDragStart={(e) =>
                    dragStart(e, { kind: "group", topicId: t.id })
                  }
                  onDragOver={(e) => e.preventDefault()}
                  onDrop={(e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    try {
                      const item = JSON.parse(
                        e.dataTransfer.getData("application/storyfold"),
                      ) as DragItem;
                      if (item.kind === "clip")
                        update((p) => ({
                          ...p,
                          sentences: p.sentences.map((s) =>
                            s.id === item.sentenceId
                              ? { ...s, topicId: t.id }
                              : s,
                          ),
                        }));
                    } catch {
                      /* unrelated drag */
                    }
                  }}
                >
                  <button
                    className="folder-select"
                    onClick={() => setFolder(t.id)}
                  >
                    <span className="folder-icon">
                      <Icon name="folder" size={23} />
                    </span>
                    <span>
                      <strong>{t.name}</strong>
                      <small>
                        {list.length} sentences ·{" "}
                        {fmt(list.reduce((n, s) => n + s.out - s.in, 0))}
                      </small>
                    </span>
                  </button>
                  <div className="folder-actions">
                    <button
                      aria-label={`Add ${t.name} folder to story`}
                      title="Add folder to story"
                      onClick={() => addItem({ kind: "group", topicId: t.id })}
                    >
                      <Icon name="plus" size={14} />
                    </button>
                    <button
                      aria-label={`Edit ${t.name} folder`}
                      onClick={() => {
                        setTopicEditor(t.id);
                        setTopicName(t.name);
                        setMergeTarget("");
                      }}
                    >
                      ···
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
          <div className="topic-bottom">
            <Icon name="folder" size={19} />
            <p>
              Drag a folder onto the timeline. Every sentence stays connected.
            </p>
          </div>
          <button
            className="button quiet small"
            onClick={() => setSyncOpen(true)}
          >
            <Icon name="settings" size={15} /> Camera sync & notes
          </button>
        </aside>
        <main className="viewer-panel">
          <div className="viewer-heading">
            <h2>{mode === "story" ? "Your story" : "A closer look"}</h2>
            <div className="segmented">
              <button
                className={mode === "sentence" ? "active" : ""}
                onClick={() => {
                  setMode("sentence");
                  setTime(0);
                  setPlaying(false);
                }}
              >
                Sentence
              </button>
              <button
                className={mode === "story" ? "active" : ""}
                disabled={!sq.total}
                onClick={() => {
                  setMode("story");
                  setTime(0);
                  setPlaying(false);
                }}
              >
                Story
              </button>
            </div>
          </div>
          <Player
            project={project}
            selected={selected}
            mode={mode}
            time={time}
            playing={playing}
            onTime={setTime}
            onPlaying={setPlaying}
          />
          <div className="playback-controls">
            <button
              className="play-circle"
              aria-label={playing ? "Pause" : "Play"}
              onClick={togglePlay}
              disabled={!duration}
            >
              <Icon name={playing ? "pause" : "play"} size={17} />
            </button>
            <span className="mono">
              {fmt(time)} <span className="muted">/ {fmt(duration)}</span>
            </span>
            <input
              aria-label="Playback position"
              type="range"
              min={0}
              max={duration || 1}
              step={0.02}
              value={time}
              onChange={(e) => {
                setTime(Number(e.target.value));
                setPlaying(false);
              }}
            />
            <span className="space-hint">space</span>
            <label className="caption-toggle">
              <input
                type="checkbox"
                checked={project.exportSettings.captions}
                onChange={(e) =>
                  update((p) => ({
                    ...p,
                    exportSettings: {
                      ...p.exportSettings,
                      captions: e.target.checked,
                    },
                  }))
                }
              />{" "}
              CC
            </label>
          </div>
          <div className="camera-heading">
            <span className="eyebrow">PICK YOUR ANGLE</span>
            <button className="text-button" onClick={autoCams}>
              Auto-switch angles <Icon name="layers" size={13} />
            </button>
          </div>
          <div className="camera-strip">
            {project.cameras.map((c) => (
              <button
                className={
                  "camera-tile " + (selected?.cam === c.id ? "active" : "")
                }
                key={c.id}
                onClick={() => editSentence({ cam: c.id })}
                disabled={!selected}
              >
                <video
                  src={`/api/projects/${project.id}/media/${c.id}#t=${Math.max(0, (selected?.in ?? 0) + c.offset)}`}
                  muted
                  preload="metadata"
                  style={{
                    transform: c.virtual
                      ? `scale(${c.virtual.scale})`
                      : undefined,
                  }}
                />
                <span className="camera-name">
                  <b>{c.id}</b>
                  {c.name}
                  {c.virtual && <small>reframe</small>}
                </span>
                {selected?.cam === c.id && (
                  <span className="camera-check">
                    <Icon name="check" size={12} />
                  </span>
                )}
              </button>
            ))}
          </div>
          {selected && (
            <section
              className="inspector"
              style={accentStyle(topic?.accent ?? "violet")}
            >
              <div className="inspector-heading">
                <h3>Make this sentence yours.</h3>
                <Badge tone={topic?.accent}>{topic?.name}</Badge>
                <span className="mono muted">
                  {fmt(selected.out - selected.in)}
                </span>
              </div>
              <div className="inspector-fields">
                <label>
                  Topic folder
                  <select
                    value={selected.topicId}
                    onChange={(e) => editSentence({ topicId: e.target.value })}
                  >
                    {project.topics.map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.name}
                      </option>
                    ))}
                  </select>
                </label>
                <label className="transcript-edit">
                  Transcript
                  <textarea
                    aria-label="Sentence transcript"
                    rows={2}
                    value={selected.text}
                    onChange={(e) => {
                      if (e.target.value.trim())
                        editSentence({ text: e.target.value });
                    }}
                  />
                </label>
              </div>
              <div
                className="waveform"
                aria-label="Waveform from your source audio"
              >
                <div className="wave-bars">
                  {wave.map((v, i) => (
                    <i
                      key={i}
                      style={{
                        height: `${Math.max(3, v * 80)}%`,
                        opacity:
                          min + ((max - min) * i) / 100 < selected.in ||
                          min + ((max - min) * i) / 100 > selected.out
                            ? 0.22
                            : 0.8,
                      }}
                    />
                  ))}
                </div>
                <div
                  className="cut-region"
                  style={{
                    left: `${((selected.in - min) / (max - min)) * 100}%`,
                    right: `${((max - selected.out) / (max - min)) * 100}%`,
                  }}
                >
                  <b />
                  <b />
                </div>
              </div>
              <div className="trim-controls">
                <label>
                  In <span className="mono">{fmt(selected.in)}</span>
                  <input
                    aria-label="Trim in"
                    type="range"
                    min={min}
                    max={Math.max(min, selected.out - 0.5)}
                    step={0.01}
                    value={selected.in}
                    onChange={(e) => {
                      editSentence({ in: Number(e.target.value) });
                      setTime(0);
                    }}
                  />
                </label>
                <button
                  className="nudge"
                  onClick={() =>
                    editSentence({ in: Math.max(min, selected.in - 0.1) })
                  }
                >
                  −0.1
                </button>
                <button
                  className="nudge"
                  onClick={() =>
                    editSentence({
                      in: Math.min(selected.out - 0.5, selected.in + 0.1),
                    })
                  }
                >
                  +0.1
                </button>
                <label>
                  Out <span className="mono">{fmt(selected.out)}</span>
                  <input
                    aria-label="Trim out"
                    type="range"
                    min={selected.in + 0.5}
                    max={max}
                    step={0.01}
                    value={selected.out}
                    onChange={(e) =>
                      editSentence({ out: Number(e.target.value) })
                    }
                  />
                </label>
                <button
                  className="nudge"
                  onClick={() =>
                    editSentence({
                      out: Math.max(selected.in + 0.5, selected.out - 0.1),
                    })
                  }
                >
                  −0.1
                </button>
                <button
                  className="nudge"
                  onClick={() =>
                    editSentence({ out: Math.min(max, selected.out + 0.1) })
                  }
                >
                  +0.1
                </button>
              </div>
              <div className="sentence-options">
                <div className="lcut-control">
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={selected.lcut > 0}
                      onChange={(e) =>
                        editSentence({ lcut: e.target.checked ? 1 : 0 })
                      }
                    />{" "}
                    L-cut into the next sentence
                  </label>
                  {selected.lcut > 0 && (
                    <label className="lcut-range">
                      <input
                        aria-label="L-cut duration"
                        type="range"
                        min={0.3}
                        max={2.5}
                        step={0.1}
                        value={selected.lcut}
                        onChange={(e) =>
                          editSentence({ lcut: Number(e.target.value) })
                        }
                      />
                      <span className="mono">{selected.lcut.toFixed(1)}s</span>
                    </label>
                  )}
                  <small>
                    The next picture comes in while this voice carries on.
                  </small>
                </div>
                <div className="skip-control">
                  <label className="check-label">
                    <input
                      type="checkbox"
                      checked={selected.skip}
                      onChange={(e) => editSentence({ skip: e.target.checked })}
                    />{" "}
                    Leave out of the story
                  </label>
                  <small>Keep it in the folder. Skip it in the cut.</small>
                </div>
              </div>
            </section>
          )}
        </main>
        <aside className="sentences-panel">
          <div className="sentence-panel-heading">
            <span className="eyebrow">THE WORDS THAT MATTER</span>
            <h2>
              {folderTopic?.name ?? "All sentences"}{" "}
              <span>{filtered.length}</span>
            </h2>
            <div className="sentence-search">
              <Icon name="search" size={16} />
              <input
                placeholder="Find a word or a moment…"
                aria-label="Search sentences"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
            </div>
            {folderTopic && (
              <button
                className="button soft small"
                onClick={() =>
                  addItem({ kind: "group", topicId: folderTopic.id })
                }
              >
                <Icon name="plus" size={15} /> Add folder to story
              </button>
            )}
          </div>
          <div className="sentence-list">
            {filtered.length ? (
              filtered.map((s) => {
                const t = project.topics.find((t) => t.id === s.topicId)!;
                return (
                  <div
                    className={
                      "sentence-card " +
                      (s.id === selectedId ? "selected" : "") +
                      (s.skip ? " skipped" : "")
                    }
                    style={accentStyle(t.accent)}
                    key={s.id}
                    draggable
                    onDragStart={(e) =>
                      dragStart(e, { kind: "clip", sentenceId: s.id })
                    }
                  >
                    <button
                      className="sentence-select"
                      onClick={() => select(s)}
                      onDoubleClick={() => select(s, true)}
                    >
                      <span className="sentence-meta">
                        <span className="grip">⠿</span>
                        <span className="mono">{fmt(s.start)}</span>
                        <span className="sentence-topic">{t.name}</span>
                      </span>
                      <span className="sentence-text">{s.text}</span>
                      <span className="sentence-tags">
                        <span>CAM {s.cam}</span>
                        {s.lcut > 0 && (
                          <span className="l-tag">
                            L-cut {s.lcut.toFixed(1)}s
                          </span>
                        )}
                        {s.skip && <span>Left out</span>}
                      </span>
                    </button>
                    <button
                      className="sentence-add"
                      aria-label={`Add sentence ${s.n} to story`}
                      title="Add sentence to story"
                      onClick={() =>
                        addItem({ kind: "clip", sentenceId: s.id })
                      }
                    >
                      <Icon name="plus" size={16} />
                    </button>
                  </div>
                );
              })
            ) : (
              <p className="empty-hint">
                No sentences match. Try another word.
              </p>
            )}
          </div>
          <div className="sentence-list-foot">
            <Icon name="plus" size={15} /> Add a sentence. Or drag it into
            place.
          </div>
        </aside>
      </div>
      <section className="timeline-section">
        <div className="timeline-toolbar">
          <h2>Story timeline</h2>
          <span className="mono muted">
            {sq.clips.length} clips · {fmt(sq.total)}
          </span>
          <div className="timeline-actions">
            <button
              className="icon-btn"
              aria-label="Undo edit"
              disabled={!undoStack.current.length}
              onClick={undo}
            >
              <Icon name="undo" size={17} />
            </button>
            <button
              className="icon-btn"
              aria-label="Redo edit"
              disabled={!redoStack.current.length}
              onClick={redo}
            >
              <Icon name="redo" size={17} />
            </button>
            <span className="toolbar-divider" />
            <label>
              Zoom
              <input
                type="range"
                aria-label="Timeline zoom"
                min={6}
                max={40}
                value={zoom}
                onChange={(e) => setZoom(Number(e.target.value))}
              />
            </label>
            {!project.timeline.length && (
              <button
                className="text-button"
                onClick={() =>
                  update((p) => ({
                    ...p,
                    timeline: p.topics.map((t) => ({
                      kind: "group",
                      topicId: t.id,
                    })),
                  }))
                }
              >
                Start with every topic <Icon name="arrow" size={14} />
              </button>
            )}
          </div>
        </div>
        <div className="timeline-body">
          <div className="track-labels">
            <span>TOPICS</span>
            <span>V1</span>
            <span>A1</span>
          </div>
          <div className="timeline-scroll">
            <div
              className="timeline-canvas"
              style={{ width: Math.max(800, sq.total * zoom + 48) }}
              onDragOver={(e) => {
                e.preventDefault();
                setDrop(insertion(e));
              }}
              onDragLeave={() => setDrop(null)}
              onDrop={dropItem}
            >
              <div
                className="timeline-ruler"
                onClick={(e) => {
                  const rect = e.currentTarget.getBoundingClientRect();
                  setMode("story");
                  setTime(
                    Math.min(
                      sq.total,
                      Math.max(0, (e.clientX - rect.left) / zoom),
                    ),
                  );
                  setPlaying(false);
                }}
              >
                {Array.from(
                  { length: Math.ceil(Math.max(sq.total, 800 / zoom) / 5) + 1 },
                  (_, i) => (
                    <span key={i} style={{ left: i * 5 * zoom }}>
                      {fmt(i * 5)}
                    </span>
                  ),
                )}
              </div>
              {!project.timeline.length ? (
                <div className="timeline-empty">
                  <span className="empty-timeline-icon">
                    <Icon name="layers" size={25} />
                  </span>
                  <strong>Every story starts somewhere.</strong>
                  <span>
                    Drag a topic folder or a sentence here to make your first
                    cut.
                  </span>
                </div>
              ) : (
                project.timeline.map((item, i) => {
                  const span = sq.spans[i],
                    t =
                      item.kind === "group"
                        ? project.topics.find((t) => t.id === item.topicId)
                        : project.topics.find(
                            (t) =>
                              t.id ===
                              project.sentences.find(
                                (s) => s.id === item.sentenceId,
                              )?.topicId,
                          );
                  return (
                    <div
                      className="timeline-topic"
                      style={{
                        ...accentStyle(t?.accent ?? "violet"),
                        left: span.start * zoom,
                        width: Math.max(28, (span.end - span.start) * zoom - 3),
                      }}
                      key={i}
                      draggable
                      onDragStart={(e) =>
                        dragStart(e, { kind: "move", index: i })
                      }
                    >
                      <span>
                        {item.kind === "group" ? t?.name : "Sentence"}
                        {!span.count ? " · empty" : ""}
                      </span>
                      <button
                        aria-label={`Move section ${i + 1} earlier`}
                        disabled={i === 0}
                        onClick={() =>
                          update((p) => {
                            const list = [...p.timeline];
                            [list[i - 1], list[i]] = [list[i], list[i - 1]];
                            return { ...p, timeline: list };
                          })
                        }
                      >
                        ‹
                      </button>
                      <button
                        aria-label={`Move section ${i + 1} later`}
                        disabled={i === project.timeline.length - 1}
                        onClick={() =>
                          update((p) => {
                            const list = [...p.timeline];
                            [list[i], list[i + 1]] = [list[i + 1], list[i]];
                            return { ...p, timeline: list };
                          })
                        }
                      >
                        ›
                      </button>
                      <button
                        aria-label={`Remove section ${i + 1}`}
                        onClick={() =>
                          update((p) => ({
                            ...p,
                            timeline: p.timeline.filter((_, n) => n !== i),
                          }))
                        }
                      >
                        ×
                      </button>
                    </div>
                  );
                })
              )}
              {sq.clips.map((c, i) => {
                const t = project.topics.find(
                  (t) => t.id === c.sentence.topicId,
                )!;
                return (
                  <div key={i} style={accentStyle(t.accent)}>
                    <button
                      className={
                        "video-clip " +
                        (c.sentence.id === selectedId ? "selected" : "")
                      }
                      style={{
                        left: c.vStart * zoom,
                        width: Math.max(3, (c.vEnd - c.vStart) * zoom - 3),
                      }}
                      title={c.sentence.text}
                      onClick={() => select(c.sentence)}
                    >
                      <b>{c.sentence.cam}</b>
                      <span>{c.sentence.text}</span>
                    </button>
                    <div
                      className="audio-clip"
                      style={{
                        left: c.aStart * zoom,
                        width: Math.max(3, c.dur * zoom - 3),
                      }}
                    >
                      <i />
                      {c.l > 0 && <b style={{ width: c.l * zoom }}>L</b>}
                    </div>
                  </div>
                );
              })}
              {drop !== null && (
                <span
                  className="drop-line"
                  style={{ left: (sq.spans[drop]?.start ?? sq.total) * zoom }}
                />
              )}
              {mode === "story" && sq.total > 0 && (
                <span className="playhead" style={{ left: time * zoom }}>
                  <b />
                </span>
              )}
            </div>
          </div>
        </div>
      </section>
      {topicEditor && (
        <Modal
          title={
            topicEditor === "new"
              ? "A new home for your words"
              : "Edit topic folder"
          }
          onClose={() => setTopicEditor(null)}
        >
          <label>
            Folder name
            <input
              autoFocus
              value={topicName}
              maxLength={80}
              onChange={(e) => setTopicName(e.target.value)}
            />
          </label>
          <button
            className="button primary"
            onClick={saveTopic}
            disabled={!topicName.trim()}
          >
            Save folder <Icon name="check" size={16} />
          </button>
          {topicEditor !== "new" && project.topics.length > 1 && (
            <div className="merge-topic">
              <h3>Bring two topics together</h3>
              <label>
                Move all sentences into
                <select
                  value={mergeTarget}
                  onChange={(e) => setMergeTarget(e.target.value)}
                >
                  <option value="">Choose a folder</option>
                  {project.topics
                    .filter((t) => t.id !== topicEditor)
                    .map((t) => (
                      <option key={t.id} value={t.id}>
                        {t.name}
                      </option>
                    ))}
                </select>
              </label>
              <p className="small muted">
                Combines matching topic sections on the timeline. You can undo
                this edit.
              </p>
              <button
                className="button secondary"
                disabled={!mergeTarget}
                onClick={mergeTopic}
              >
                Merge folders
              </button>
            </div>
          )}
        </Modal>
      )}
      {syncOpen && (
        <Modal
          title="Your cameras & processing notes"
          onClose={() => setSyncOpen(false)}
        >
          <p className="muted">
            Camera A is the audio master. Offsets add seconds to story source
            time for each camera.
          </p>
          {project.cameras.map((c) => (
            <label key={c.id}>
              Camera {c.id} · {c.name}
              {c.virtual ? " (virtual crop)" : null}
              <input
                type="number"
                step={0.01}
                disabled={c.id === "A" || !!c.virtual}
                value={c.offset}
                onChange={(e) =>
                  update((p) => ({
                    ...p,
                    cameras: p.cameras.map((x) =>
                      x.id === c.id
                        ? { ...x, offset: Number(e.target.value) }
                        : x,
                    ),
                  }))
                }
              />
            </label>
          ))}
          {project.warnings.map((w, i) => (
            <p className="note" key={i}>
              {w}
            </p>
          ))}
          <small>{project.grouping}</small>
        </Modal>
      )}
      {exportOpen && (
        <Modal
          title={
            exportJob?.status === "done"
              ? "Your story is ready."
              : "Make it a story worth sharing."
          }
          onClose={() => setExportOpen(false)}
        >
          <p className="muted">
            {fmt(sq.total)} · {sq.clips.length} clips · saved from revision{" "}
            {revision.current}
          </p>
          <ErrorBanner error={exportJob?.error ?? error} />
          {exportJob && ["queued", "rendering"].includes(exportJob.status) ? (
            <div className="export-progress">
              <span className="spinner" />
              <h3>{exportJob.detail}</h3>
              <progress max={100} value={exportJob.pct} />
              <span className="mono">{exportJob.pct}%</span>
              <p className="small muted">
                You can close this dialog while your export finishes.
              </p>
            </div>
          ) : exportJob?.status === "done" ? (
            <>
              <div className="download-grid">
                {exportJob.files.map((file) => (
                  <a
                    className={
                      "button " +
                      (file.endsWith(".mp4") ? "primary" : "secondary")
                    }
                    href={`/api/projects/${project.id}/exports/${exportJob.id}/files/${file}`}
                    key={file}
                  >
                    <Icon name="download" size={17} />
                    {file.endsWith(".mp4")
                      ? "Download your video"
                      : file.replace("story.", "").toUpperCase()}
                  </a>
                ))}
              </div>
              <p className="note small">
                EDL uses 25 fps and includes virtual-crop notes. FCPXML
                references source filenames; relink your footage in your NLE.
                NLE import still needs validation.
              </p>
              <button
                className="button quiet"
                onClick={() => setExportJob(null)}
              >
                Make another export
              </button>
            </>
          ) : (
            <>
              <label>
                Frame your story
                <select
                  value={project.exportSettings.ratio}
                  onChange={(e) =>
                    update((p) => ({
                      ...p,
                      exportSettings: {
                        ...p.exportSettings,
                        ratio: e.target
                          .value as Project["exportSettings"]["ratio"],
                      },
                    }))
                  }
                >
                  <option value="16:9">16:9 · landscape</option>
                  <option value="9:16">9:16 · portrait</option>
                  <option value="1:1">1:1 · square</option>
                </select>
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={project.exportSettings.captions}
                  onChange={(e) =>
                    update((p) => ({
                      ...p,
                      exportSettings: {
                        ...p.exportSettings,
                        captions: e.target.checked,
                      },
                    }))
                  }
                />{" "}
                Burn captions into the video
              </label>
              <label className="check-label">
                <input
                  type="checkbox"
                  checked={project.exportSettings.chapters}
                  onChange={(e) =>
                    update((p) => ({
                      ...p,
                      exportSettings: {
                        ...p.exportSettings,
                        chapters: e.target.checked,
                      },
                    }))
                  }
                />{" "}
                Add a chapter for each story section
              </label>
              <p className="small muted">
                MP4 + SRT captions + EDL + FCPXML. Rendered locally from your
                original footage.
              </p>
              <button
                className="button primary"
                disabled={exportBusy || !sq.total}
                onClick={() => void startExport()}
              >
                {exportBusy ? "Saving your latest edits…" : "Export story"}
                <Icon name="arrow" size={16} />
              </button>
            </>
          )}
        </Modal>
      )}
    </div>
  );
}
