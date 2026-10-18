# Storyfold

Edit interviews by what people say. Topic folders and sentence clips become a
story timeline, with trims, camera choices and L-cuts. **Catalyst Solo** is the
local panel for seeing progress, editing your build, reviewing source changes
and undoing them through Git.

## Run it

Requires Node **22.12+**, Python **3.9+** with `venv`, Git, and
`ffmpeg` / `ffprobe` on PATH.

```sh
npm ci
npm run setup:whisper
npm run dev
```

- [Storyfold editor](http://127.0.0.1:5173)
- [Catalyst Solo — see and control the build](http://127.0.0.1:5173/catalyst)

For the production build: `npm run build`, then `npm start`; both views are
served at `http://127.0.0.1:4310`. Bind stays on loopback. No Docker is needed.

## Try an interview

**Open the sample story** works without credentials. Its synthetic test pattern,
quiet tone and supplied transcript are labeled; it does not prove speech
recognition or camera alignment.

Upload 1–3 videos, each no longer than two hours (4 GiB per uploaded file).
Whisper automatically transcribes camera A and generates word timestamps;
no transcript JSON is required. Run `npm run setup:whisper` once to install
the isolated Python runtime and download the small model (about 500 MB).
Setup needs internet; subsequent local transcription runs offline on the CPU.
Refresh the app after setup and look for **Local Whisper** readiness.

For Claude topic folders, configure
`CLAUDE_API_KEY` (or `ANTHROPIC_API_KEY`) in that private file. Claude is selected
when its key is present. The backend reads the file on startup; restart after
changing keys. Keep example files empty and never commit credentials.

Claude uses `claude-haiku-5-5` by default; `CLAUDE_MODEL` overrides it.
`TOPIC_MODEL` applies to OpenAI and defaults to `gpt-4.1-mini`.

**Organize topics myself** skips AI topic grouping while Whisper still
transcribes automatically. An existing transcript can optionally be imported
under **Use an existing transcript**. It has this shape, with seconds on the
camera A clock:

```json
{
  "words": [
    { "text": "Hello,", "start": 0.2, "end": 0.7 },
    { "text": "world.", "start": 0.75, "end": 1.5 }
  ]
}
```

Local Whisper keeps interview audio on this computer. If local Whisper is not
installed and `OPENAI_API_KEY` is configured, the existing OpenAI `whisper-1`
fallback sends camera A audio to OpenAI; the upload form identifies the active
speech provider. Automatic topics send transcript text to Claude or OpenAI,
as shown in the upload form. Imported transcripts skip speech recognition.
The sample and manual-topic transcript import make no provider calls.
Without a topic provider, organize folders manually. The local worker uses
[faster-whisper](https://github.com/SYSTRAN/faster-whisper) with word timestamps.

Projects, original footage, proxies, word timestamps, waveforms and exports live
under ignored `data/projects/`. Set `STORYFOLD_DATA_DIR` to use another disk.
Processing reports real progress and can resume from persisted outputs after an
interruption. Exports retain a snapshot of the project revision they rendered.

## Control your build

Catalyst shows real source files, local Git history, tasks and build output.
**Content** opens the welcome copy, **Design** opens the stylesheet, and **Code**
opens other supported text files. Draft → Review → Apply makes one scoped Git
commit; **History → Review undo** restores the prior content as a new commit.
Stale proposals are rejected rather than overwriting newer work.

**Tasks & roadmap** lets you change priorities and define “done”. Edit
`.catalyst/instructions.md` to leave durable build instructions. Progress lives
in `.catalyst/pipeline.json`; the coding agent records it during a build.

**Pause pipeline** prevents new local test/build runs. **Cancel run** terminates
an active check. Use chat to steer or stop Codex itself. The preview runs the
actual app at desktop, tablet or phone width. Publishing and Git auto-push are
not enabled for this local release.

```sh
npm run catalyst -- status
npm run catalyst -- progress "A plain-language progress update"
npm run catalyst -- checks build
npm run catalyst -- pause
npm run catalyst -- resume
```

## Verification

```sh
npm run typecheck
npm run lint
npm test
python3 -m unittest discover -s tests -p test_whisper.py
npm run build
npm run test:media
# With npm run dev running in another terminal:
npx playwright install chromium
npm run test:e2e
```

`test:media` generates a disposable red/blue video with two different tones. It
renders portrait MP4 and checks that the picture changes at 2s, the audio at 3s,
the duration is 6s, and captions/chapters are present. Results are recorded in
`docs/validation/`. Browser tests save screenshots under `test-results/`.

The opt-in Whisper browser check generates speech locally, uploads only its
video and verifies recognized words, word timestamps and an editable project.
It requires completed Whisper setup, the running app and FFmpeg with `flite`:

```sh
STORYFOLD_TEST_WHISPER=1 npm run test:e2e -- tests/e2e/whisper.spec.ts
```

Its evidence is `docs/validation/whisper-smoke.json`. This short synthesized
clip does not measure real-interview accuracy.

The [demo checklist](docs/demos/LOCAL-RELEASE.md) describes the user flow.
[Architecture](docs/ARCHITECTURE.md), [decisions](docs/DECISIONS.md) and
[security boundaries](docs/SECURITY.md) document the implementation.

## Current limits

This is the first working local release. Real-interview transcription accuracy,
one-frame multicamera alignment, gap-free playback on slow disks, NLE import,
Windows installers and publishing remain to be validated. Automatic topic
classification currently handles up to 400 sentences; longer imports can use
manual topics. Audio cuts use 5ms fades to avoid clicks, rather than the brief's
10ms crossfade. EDL records virtual crops as notes; FCPXML requires relinking
source files and has not been verified in Final Cut Pro.

Ponytail is installed as a Codex skill and was used for this build. Its upstream
is [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail).
Doberman Core 0.18.7 and its user-wide Codex hook are installed and enforcing.
The approved file-patch compatibility repair is applied; policy remains enabled.
Review `/hooks` if Codex reports a changed definition needing trust. See
[the installation record](docs/DOBERMAN.md).
