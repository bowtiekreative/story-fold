# Validation record — 2026-10-15

## Long interview grouping — 2026-10-18

- Removed the 400-sentence rejection. Grouping now discovers shared folders
  from excerpts spanning the whole interview, then assigns full sentence text
  in requests bounded to 300 sentences and 60,000 sentence-text characters.
  Structured batch output permits only the discovered folder IDs.
- All assignments must cover the requested sentence IDs exactly once. Each
  request gets one retry. Results are applied together; failed classification
  leaves the whole transcript in a consistent, editable manual folder.
- 29 unit/integration tests, typecheck, ESLint and the frontend build pass.
  New checks cover a 1,001-sentence interview, character-budget batching without
  truncated assignment text, shared folder IDs, targeted retries, monotonic
  progress, preserved speech timing and a failure after an earlier batch passes.
- The saved interview that hit the old limit has 612 sentences and 11,647
  cached words. Its transcription is still present. An automatic-resume helper
  under `/tmp` was refused by Doberman's repository-boundary and high-entropy
  checks and was not recreated elsewhere. Refresh the project and select
  **Resume processing** to reuse those saved words and media outputs.
- Live Claude Haiku 5.5 validation passed on 612 generated sentences in 67.3
  seconds: four shared folders, three assignment batches, complete sentence
  coverage and unchanged timing. See [topic-batches.json](topic-batches.json).
  No private interview text was sent by this test or included in artifacts.
  The saved interview itself still awaits the user's Resume action. Blue
  progress sync remains blocked by the guard.

## Automatic Whisper update — 2026-10-17

- **Installed and speech-tested after Ryan's explicit approval.** The setup
  command completed and the app now reports Local Whisper ready. Its isolated
  runtime uses faster-whisper 1.2.1 and PyAV 18.1.0 with the small CPU model.
- The live browser upload supplied only a locally synthesized 13.485-second
  video, with manual topic grouping. Whisper generated 41 words with finite,
  ordered source timestamps (0–13.04 seconds) and an editable sentence clip.
  All five expected content keywords were recognized. No imported transcript,
  private footage or cloud speech API was used. The complete check passed in
  20.1 seconds; evidence is [whisper-smoke.json](whisper-smoke.json).
- The first live check found that the initially resolved PyAV 19.0.1 removed
  an argument used by faster-whisper 1.2.1. Setup now constrains PyAV below 19
  and decodes a silent WAV before reporting ready. Three Python worker checks
  pass, including refusal to report readiness when the decoder is incompatible.
  Typecheck and ESLint pass after the repair. The
  [upstream decoder](https://github.com/SYSTRAN/faster-whisper/blob/master/faster_whisper/audio.py)
  documents the same PyAV 19 API change.
- Video uploads now use automatic word-timestamp transcription. Timed JSON is
  an optional import. Manual topics skips only AI grouping, not transcription.
- Added an isolated local faster-whisper setup command and offline CPU worker.
  The server prefers completed local setup, retains the existing OpenAI Whisper
  fallback, validates timed output and checkpoints each audio chunk for resume.
- Typecheck, ESLint, the frontend build and 25 JavaScript tests passed. Two
  Python worker tests passed using mocked model responses; they verify offline
  loading, decoding settings and word-timestamp JSON, not recognition accuracy.
- The setup script passed Node syntax checking. The Python worker compiled.
- Seven browser checks passed across runs: video-only submission without a
  transcript, actionable malformed-import errors, sample editing/export,
  Catalyst source review, phone layouts, imported transcripts and stale saves,
  and reviewed Git apply/undo. The Git apply/undo check was rerun after removing
  a confirmed unowned stale index lock and reverting its interrupted test edit.
  The dev preview was restarted with polling after the system watcher limit
  prevented its backend from reloading; the current backend passes the checks.
- Doberman refused the package download with `egress_requires_auth` and
  `correlated_trifecta`; its approval dialog cannot open in this session. The
  download was not retried through another tool or wrapper. The Python virtual
  environment initially contained only pip. Ryan was given the terminal
  command, then explicitly approved installation in chat. The authorized setup
  succeeded and the live validation above replaces that installation blocker.
- A proposed CI workflow edit was separately refused as a sensitive path and
  left unchanged. The Python checks are documented for manual execution.

Local speech recognition and video-only processing now pass with synthesized
speech. Real-interview accuracy, noisy audio, long recordings and multicamera
alignment remain unverified. This update supersedes the Claude-only timed-import
requirement below. Blue progress sync remains blocked by the guard.

## Claude update — 2026-10-17

- Applied the explicitly approved Doberman 0.18.7 adapter repair. Installed and
  backup hashes match the tested candidate and original. Native source edits
  now pass; protected example-config edits are still refused.
- Claude is selected when its private key is configured. Both supported key
  names work. OpenAI remains available for speech transcription and as the
  fallback topic provider when no Claude key is configured.
- A live standalone Claude Haiku 5.5 structured-output request produced four
  folders and assigned all four synthetic sentences. Authentication returned
  HTTP 200. No private interview content was sent.
- Typecheck, lint, the frontend build and 23 unit/integration tests passed.
  New tests cover provider selection, request format, key redaction, incomplete
  responses, application grouping, preserved speech timing and manual fallback.
  Application grouping tests use mocked provider responses.
- Six browser flows passed, including Claude readiness, required timed words,
  local manual grouping, sample editing/export, review/undo and phone layouts.
  The actual welcome screenshot shows Claude configured with the private key;
  the backend restarted successfully after the integration changes.
- A separate live application-grouping probe was refused by the remaining
  high-entropy rule. It was not rerouted; live end-to-end grouping remains
  unverified. The same rule blocked Blue progress sync. Policy remains enabled.

Claude handles topic text. A Claude-only setup still needs an imported timed
transcript alongside video. Audio transcription with a real interview and the
full production release remain open.

## Original local release

Verified locally on Linux with Node 22.23.1 and ffmpeg 6.1.1.

- TypeScript strict typecheck and ESLint passed.
- 17 unit/integration tests passed: live topic membership and repeated clips,
  L-cut continuity and duration clamps, sentence splitting and long gaps,
  timestamp validation, timecodes, interchange generation, source protection,
  stale review detection, Git commit/undo and preserved unrelated staged work,
  known audio-envelope offset, immutable media fields and concurrent saves.
- Five browser flows passed: sample processing/editing/autosave/reload/playback/
  MP4 download; Catalyst source review/discard/preview/pause; phone layout;
  uploaded video + timed transcript + simultaneous stale-save rejection; a real
  repository file review/apply/commit followed by reviewed undo.
- `media-smoke.json`: six-second, 720×1280 MP4, two chapters, burned white caption
  pixels, picture changes at 2s while audio stays at 440Hz until 3s and then
  changes to 880Hz. The virtual B crop is rendered too.
- Production frontend build passed.
- Doberman package 0.18.7 and isolated hook-adapter allow/deny fixtures verified;
  `doberman-smoke.json`. No live Codex hook activation is claimed.

The real-repository browser apply/undo test passed after feature commit
`bdddc7e`. Commits `195cdfd` and `8d723c5` record the applied practice edit and
its undo. `docs/demos/REVIEW-CHECK.md` is restored exactly. The original reference
prototypes remain untouched and untracked, as they were at the start.

CI is configured for Ubuntu and Windows core checks and Ubuntu media/browser
checks. It has not run remotely; no remote CI result is claimed. No commit was
pushed and no deployment was created.

Not verified: API-key-backed transcription and topic quality, a real interview's
95% accuracy / 150ms timing targets, multicamera clap alignment, playback under
slow-disk or large-project load, Resolve/Premiere/Final Cut import, desktop
installers, general Astro/Next/WordPress management, host publishing and rollback.
