# Storyfold and Catalyst Solo

Storyfold is a local web app for editing interviews by sentences and topic folders.
The supplied `BUILD_PROMPT.md` and `Storyfold Editor.dc.html` define its behavior
and appearance. Catalyst Solo is the build and maintenance panel at `/catalyst`.
Its supplied handoff defines the review → apply → Git commit → undo workflow.

## First working release

- `apps/web`: React, strict TypeScript and Vite; the editor and control panel.
- `apps/server`: Express and TypeScript; local storage, media jobs, progress
  events, a repository scanner and reviewed file edits.
- `packages/timeline`: pure sentence, timing and interchange export functions
  shared by the browser and renderer.
- `packages/schema/site.schema.json`: Catalyst Site Map schema.
- `.catalyst/site.json`: discovered routes and editable source references.
- `.catalyst/pipeline.json`: durable build tasks, acceptance criteria and progress.
- `data/projects/<id>`: uploaded footage, proxies, transcript and project state.

One Node service binds to `127.0.0.1`. During development Vite proxies `/api` to
it; production serves the built frontend from the same origin. SSE reports actual
job and build progress. Disk writes use temporary files and rename. A revision
check prevents stale clients from overwriting newer project changes.

## Media flow

Upload → ffprobe → proxy + waveform → automatic Whisper (or imported timed words)
→ sentence splitting → topic classification → editor. Missing provider setup is
reported explicitly. A labeled synthetic sample exercises the pipeline offline.
Camera A is the audio master; virtual B/C crops use the same file. Additional
real cameras require verified offsets before the project is ready.

`npm run setup:whisper` prepares `data/runtime/whisper` and the small model in
`data/models/whisper-small`. A readiness marker is written only after the model
loads successfully. The server spawns `scripts/whisper.py` offline on CPU with
int8 computation, VAD and word timestamps. It processes ten-minute audio chunks
with a one-second overlap and persists validated chunk results so interrupted
processing can resume. The existing OpenAI Whisper API is used only when local
Whisper is unavailable and an OpenAI key is configured. Claude handles topic
grouping independently of speech recognition.

Setup also decodes a small silent WAV before reporting readiness. PyAV is
constrained below 19 because faster-whisper 1.2.1 calls an option removed in
that release; a successful model load alone did not catch this incompatibility.

The shared timeline flattens live topic groups and individual sentences and
computes separate audio and video intervals for L-cuts. Preview, MP4 rendering,
SRT and EDL all use those same intervals. Media remains local except audio or
transcript explicitly submitted to the configured transcription/topic provider.

## Build visibility and control

Catalyst scans this repository, displays tasks and recorded evidence, serves a
real preview and exposes editable text files. File edits become immutable review
proposals with a content hash. Applying verifies the original bytes, scans for
secrets, writes the file and creates a path-scoped Git commit. Undo is a new
reviewed change and refuses to overwrite intervening changes. Git operations
use an isolated index so unrelated staged or untracked user work is preserved.

Only fixed check commands run from the dashboard. Pause stops new check runs;
Cancel terminates an active run. These controls govern the local pipeline, not
the Codex conversation. Build instructions and tasks are editable; steering a
currently running Codex turn also uses chat.

## Boundaries

The server rejects non-local Host/Origin values, requires a per-process token for
mutations, restricts files to this project, blocks symlinks and private paths,
and never returns environment values. It is a trusted single-user development
tool, not a public multi-user service. There is no arbitrary shell endpoint.
Desktop installers, publishing adapters and other stacks remain later Catalyst
milestones. Do not call those milestones complete from this first release.

## Verification

Unit tests cover timeline invariants, splitting and timecodes. Integration tests
use disposable projects/repositories for persistence, path boundaries, review,
commit and undo. Browser acceptance covers the sample editor and control panel.
An ffmpeg smoke test probes a rendered L-cut export. A supplied real interview is
still required to measure transcription accuracy and multicamera alignment.
