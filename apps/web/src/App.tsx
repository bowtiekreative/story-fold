import { useState, useEffect, useRef } from "react";
import type { Project } from "../../../packages/timeline/index.ts";
import { api, send } from "./api.ts";
import { Mark, Icon, Badge, ErrorBanner, useEvents } from "./ui.tsx";
import { Editor } from "./Editor.tsx";
import copy from "./copy.json";
type Summary = {
  id: string;
  name: string;
  status: string;
  sentences: number;
  createdAt: string;
  sample: boolean;
};
export function App() {
  const [project, setProject] = useState<Project | null>(null),
    [projects, setProjects] = useState<Summary[]>([]),
    [id, setId] = useState(
      new URLSearchParams(location.search).get("project") ?? "",
    );
  const [error, setError] = useState(""),
    [busy, setBusy] = useState(false),
    [files, setFiles] = useState<File[]>([]),
    [transcript, setTranscript] = useState<File | null>(null),
    [name, setName] = useState(""),
    [manual, setManual] = useState(false),
    [drag, setDrag] = useState(false),
    [providers, setProviders] = useState({
      transcription: false,
      speechProvider: "Checking",
      topics: false,
      topicProvider: "Manual",
    });
  const picker = useRef<HTMLInputElement>(null);
  const transcriptPicker = useRef<HTMLInputElement>(null);
  async function refresh() {
    try {
      setProjects(await api<Summary[]>("/projects"));
      if (id) setProject(await api<Project>("/projects/" + id));
    } catch (e) {
      setError((e as Error).message);
    }
  }
  useEffect(() => {
    void refresh();
  }, [id]);
  useEffect(() => {
    void api<typeof providers>("/health")
      .then(setProviders)
      .catch(() => {});
  }, []);
  useEvents(() => {
    if (id && project?.status !== "ready") void refresh();
  });
  function open(next: string) {
    setId(next);
    setProject(null);
    setError("");
    history.replaceState(null, "", next ? "/?project=" + next : "/");
  }
  async function sample() {
    setBusy(true);
    setError("");
    try {
      const p = await send<{ projectId: string }>("/projects/sample");
      open(p.projectId);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  function choose(next: File[]) {
    if (next.length > 3) {
      setError("Choose up to three camera files.");
      return;
    }
    setFiles(next);
    if (!name && next[0]) setName(next[0].name.replace(/\.[^.]+$/, ""));
    setError("");
  }
  async function upload() {
    if (!files.length) return;
    setBusy(true);
    setError("");
    try {
      const form = new FormData();
      files.forEach((f) => form.append("files", f));
      if (transcript) form.append("transcript", transcript);
      form.append("name", name || "Untitled interview");
      form.append("grouping", manual ? "manual" : "auto");
      const p = await api<{ projectId: string }>("/projects", {
        method: "POST",
        body: form,
      });
      open(p.projectId);
    } catch (e) {
      setError((e as Error).message);
    } finally {
      setBusy(false);
    }
  }
  if (project?.status === "ready")
    return (
      <Editor key={project.id} initial={project} onHome={() => open("")} />
    );
  const processing = !!id;
  return (
    <div className="app-shell">
      <header className="app-header">
        <a
          className="brand"
          href="/"
          onClick={(e) => {
            e.preventDefault();
            open("");
          }}
        >
          <Mark />
          <strong>Storyfold</strong>
          <span className="brand-divider" />
          <span className="brand-sub">
            A little less footage. A lot more story.
          </span>
        </a>
        <a className="button quiet" href="/catalyst">
          <Icon name="layers" size={17} /> Your build{" "}
          <span className="live-dot" />
        </a>
      </header>
      {processing ? (
        <main className="processing">
          <span className="eyebrow">YOUR WORDS, TAKING SHAPE</span>
          <h1>
            {project?.status === "error"
              ? "Let’s pick up where we left off."
              : "Listening to every word."}
          </h1>
          <p className="lede">{project?.name ?? "Opening your interview…"}</p>
          <ErrorBanner error={project?.error ?? error} />
          <div className="processing-card">
            {[
              "uploading",
              "transcribing",
              "segmenting",
              "syncing",
              "grouping",
              "ready",
            ].map((step, i) => {
              const events = project?.progress ?? [],
                current = events.at(-1);
              const done =
                events.some((e) => e.step === step) && current?.step !== step;
              return (
                <div
                  className={
                    "process-step " + (current?.step === step ? "current" : "")
                  }
                  key={step}
                >
                  <span className={"step-icon " + (done ? "done" : "")}>
                    {done ? <Icon name="check" size={16} /> : i + 1}
                  </span>
                  <div>
                    <strong>
                      {
                        [
                          "Getting your footage ready",
                          "Transcribing speech",
                          "Finding each sentence",
                          "Lining up your angles",
                          "Making topic folders",
                          "Your story is ready",
                        ][i]
                      }
                    </strong>
                    <small>
                      {events.findLast((e) => e.step === step)?.detail ??
                        "Waiting for the previous step"}
                    </small>
                  </div>
                  {current?.step === step && project?.status !== "error" && (
                    <span className="spinner" />
                  )}
                </div>
              );
            })}
            <progress max={100} value={project?.progress.at(-1)?.pct ?? 0} />
          </div>
          {project?.status === "error" && (
            <button
              className="button primary"
              onClick={() => {
                setError("");
                void send("/projects/" + id + "/resume")
                  .then(() => refresh())
                  .catch((e) => setError(e.message));
              }}
            >
              Resume processing <Icon name="arrow" />
            </button>
          )}
          <button className="button quiet" onClick={() => open("")}>
            Back to your interviews
          </button>
        </main>
      ) : (
        <>
          <main className="welcome">
            <div className="welcome-copy">
              <span className="eyebrow">
                <span className="small-line" /> LESS SCRUBBING. MORE
                STORYTELLING.
              </span>
              <h1>
                {copy.headline}
                <br />
                <span>{copy.headlineAccent}</span>
              </h1>
              <p className="lede">{copy.intro}</p>
              <div className="feature-pills">
                <span>
                  <Icon name="film" size={16} /> Edit by sentence
                </span>
                <span>
                  <Icon name="folder" size={16} /> Find every topic
                </span>
                <span>
                  <Icon name="layers" size={16} /> Keep every angle
                </span>
              </div>
              <div
                className={"upload-card " + (drag ? "dragging" : "")}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDrag(true);
                }}
                onDragLeave={() => setDrag(false)}
                onDrop={(e) => {
                  e.preventDefault();
                  setDrag(false);
                  choose([...e.dataTransfer.files]);
                }}
              >
                <input
                  ref={picker}
                  type="file"
                  accept="video/*,.mkv,.mov,.mp4,.webm,.mxf"
                  multiple
                  hidden
                  onChange={(e) => choose([...(e.target.files ?? [])])}
                />
                <div className="upload-card-top">
                  <span className="upload-symbol">
                    <Icon name="upload" size={24} />
                  </span>
                  <div>
                    <h2>
                      {files.length
                        ? `${files.length} camera${files.length === 1 ? "" : "s"} ready to go`
                        : copy.uploadTitle}
                    </h2>
                    <p>
                      {files.length
                        ? files.map((f) => f.name).join(" · ")
                        : copy.uploadHint}
                    </p>
                  </div>
                </div>
                <div className="provider-readiness" role="status">
                  <Badge tone={providers.topics ? "teal" : "amber"}>
                    {providers.topics
                      ? `${providers.topicProvider} topics configured`
                      : "Manual topics available"}
                  </Badge>
                  <small>
                    {providers.transcription
                      ? `${providers.speechProvider} transcribes your video automatically.`
                      : providers.speechProvider === "Checking"
                        ? "Checking automatic transcription…"
                        : "Automatic Whisper transcription needs one-time setup."}
                  </small>
                  {!providers.transcription &&
                    providers.speechProvider !== "Checking" && (
                      <small>
                        Run <code>npm run setup:whisper</code> once in the
                        project terminal, then refresh this page.
                      </small>
                    )}
                </div>
                {files.length > 0 && (
                  <div className="upload-options">
                    <label>
                      Interview name
                      <input
                        value={name}
                        onChange={(e) => setName(e.target.value)}
                        maxLength={120}
                      />
                    </label>
                    <details className="transcript-import">
                      <summary>Use an existing transcript (optional)</summary>
                      <label>
                        Timed transcript JSON (optional)
                        <input
                          ref={transcriptPicker}
                          type="file"
                          accept=".json"
                          onChange={(e) =>
                            setTranscript(e.target.files?.[0] ?? null)
                          }
                        />
                      </label>
                      {transcript && (
                        <button
                          className="button quiet"
                          onClick={() => {
                            setTranscript(null);
                            if (transcriptPicker.current)
                              transcriptPicker.current.value = "";
                          }}
                        >
                          Use automatic Whisper instead
                        </button>
                      )}
                      <small>
                        Timed JSON:{" "}
                        {'{"words":[{"text":"Hello.","start":0.2,"end":1.1}]}'}
                      </small>
                    </details>
                    <label className="check-label">
                      <input
                        type="checkbox"
                        checked={manual}
                        onChange={(e) => setManual(e.target.checked)}
                      />{" "}
                      Organize topics myself
                    </label>
                    <small>
                      {transcript
                        ? "Using your imported transcript; audio stays local. "
                        : providers.speechProvider === "Local Whisper"
                          ? "Whisper transcribes audio on this computer. "
                          : providers.speechProvider === "OpenAI Whisper"
                            ? "Whisper transcribes speech using the OpenAI audio API. "
                            : "Whisper will create the transcript after setup. "}
                      {!manual && providers.topics
                        ? `Transcript text is sent to ${providers.topicProvider} for topic suggestions.`
                        : "Organize topics locally."}
                    </small>
                  </div>
                )}
                <ErrorBanner error={error} />
                <div className="upload-actions">
                  <button
                    className="button primary"
                    disabled={busy}
                    onClick={() =>
                      files.length ? void upload() : picker.current?.click()
                    }
                  >
                    {busy
                      ? "Getting things ready…"
                      : files.length
                        ? copy.primaryAction
                        : "Choose your footage"}
                    <Icon name="arrow" size={18} />
                  </button>
                  {files.length > 0 ? (
                    <button
                      className="button quiet"
                      disabled={busy}
                      onClick={() => picker.current?.click()}
                    >
                      Change files
                    </button>
                  ) : (
                    <span>or drop your files here</span>
                  )}
                </div>
                <div className="upload-foot">
                  <span className="live-dot" /> Saved on this computer{" "}
                  <span>MP4, MOV, WebM + more · up to 2 hours</span>
                </div>
              </div>
              <button
                className="sample-link"
                disabled={busy}
                onClick={() => void sample()}
              >
                <span className="sample-play">
                  <Icon name="play" size={12} />
                </span>{" "}
                Just looking? Open the sample story.{" "}
                <Icon name="arrow" size={16} />
              </button>
              <small className="sample-note">
                Synthetic footage with sample text. No account needed.
              </small>
            </div>
            <div
              className="story-art"
              aria-label="Illustration of sentences organized into topic folders"
            >
              <div className="art-grid" />
              <div className="art-caption">
                <span className="live-dot" /> A GOOD STORY STARTS WITH A FEW
                GOOD WORDS.
              </div>
              <div className="art-video">
                <div className="art-window-dots">
                  <i />
                  <i />
                  <i />
                  <span>THE BAKERY · INTERVIEW 01</span>
                </div>
                <div className="art-scene">
                  <div className="sunlight" />
                  <div className="window-pane" />
                  <div className="arch" />
                  <div className="table" />
                  <span className="art-record">
                    <i /> CAM A <span>00:12.8</span>
                  </span>
                  <div className="art-quote">
                    “My mom was up at four,
                    <br />
                    every single day.”
                  </div>
                </div>
                <div className="art-video-bottom">
                  <span>
                    <Icon name="play" size={12} /> A story, waiting to be told
                  </span>
                  <span>01 / 08</span>
                </div>
              </div>
              <div className="art-folder coral">
                <Icon name="folder" size={26} />
                <div>
                  <strong>Mom</strong>
                  <small>Every little thing she taught me.</small>
                </div>
                <span>02</span>
              </div>
              <div className="art-folder teal">
                <Icon name="folder" size={26} />
                <div>
                  <strong>The bakery</strong>
                  <small>It always comes back to the bread.</small>
                </div>
                <span>02</span>
              </div>
              <div className="art-sentence">
                <span className="mono">00:27.3</span>
                <p>“If you love someone, show up early.”</p>
                <Badge tone="violet">Advice</Badge>
                <span className="art-add">+</span>
              </div>
              <div className="art-timeline">
                <span className="mono">YOUR STORY</span>
                <div>
                  <i />
                  <i />
                  <i />
                  <i />
                  <i />
                </div>
                <span className="art-cursor" />
              </div>
              <span className="art-note">
                Move the words.
                <br />
                Make it yours.
                <svg width="68" height="30" viewBox="0 0 68 30">
                  <path
                    d="M65 2C40 5 36 25 6 18m6-6-6 6 8 5"
                    stroke="currentColor"
                    fill="none"
                    strokeWidth="1.5"
                  />
                </svg>
              </span>
            </div>
          </main>
          <section className="recent-section">
            <div className="section-heading">
              <div>
                <span className="eyebrow">PICK UP THE THREAD</span>
                <h2>Your interviews</h2>
              </div>
              <span className="muted small">Your footage stays yours.</span>
            </div>
            {projects.length ? (
              <div className="project-grid">
                {projects.map((p) => (
                  <button
                    className="project-card"
                    key={p.id}
                    onClick={() => open(p.id)}
                  >
                    <span
                      className={
                        "project-icon " + (p.sample ? "amber" : "violet")
                      }
                    >
                      <Icon name="film" size={22} />
                    </span>
                    <div>
                      <strong>{p.name}</strong>
                      <small>
                        {p.sample
                          ? "Synthetic sample"
                          : p.sentences + " sentences"}{" "}
                        · {new Date(p.createdAt).toLocaleDateString()}
                      </small>
                    </div>
                    <Badge tone={p.status === "error" ? "coral" : "violet"}>
                      {p.status === "ready" ? "Open story" : p.status}
                    </Badge>
                    <Icon name="chevron" size={16} />
                  </button>
                ))}
              </div>
            ) : (
              <div className="empty-projects">
                <Icon name="film" size={24} />
                <span>
                  Your next good story starts here. Add an interview or try the
                  sample.
                </span>
              </div>
            )}
          </section>
          <footer className="welcome-footer">
            <span>
              <Mark /> Made for the stories only you can tell.
            </span>
            <a href="/catalyst">
              Built with care. Controlled by you.{" "}
              <Icon name="arrow" size={14} />
            </a>
          </footer>
        </>
      )}
    </div>
  );
}
