# Validation record — 2026-10-15

Verified locally on Linux with Node 22.23.1 and ffmpeg 6.1.1.

- TypeScript strict typecheck and ESLint passed.
- 17 unit/integration tests passed: live topic membership and repeated clips,
  L-cut continuity and duration clamps, sentence splitting and long gaps,
  timestamp validation, timecodes, interchange generation, source protection,
  stale review detection, Git commit/undo and preserved unrelated staged work,
  known audio-envelope offset, immutable media fields and concurrent saves.
- Four browser flows passed: sample processing/editing/autosave/reload/playback/
  MP4 download; Catalyst source review/discard/preview/pause; phone layout;
  uploaded video + timed transcript + simultaneous stale-save rejection.
- `media-smoke.json`: six-second, 720×1280 MP4, two chapters, burned white caption
  pixels, picture changes at 2s while audio stays at 440Hz until 3s and then
  changes to 880Hz. The virtual B crop is rendered too.
- Production frontend build passed.
- Doberman package 0.18.7 and isolated hook-adapter allow/deny fixtures verified;
  `doberman-smoke.json`. No live Codex hook activation is claimed.

The real-repository browser apply/undo flow is checked after the feature commit
and recorded in this file when it passes. The unit Git test already passes in a
disposable repository.

CI is configured for Ubuntu and Windows core checks and Ubuntu media/browser
checks. It has not run remotely; no remote CI result is claimed. No commit was
pushed and no deployment was created.

Not verified: API-key-backed transcription and topic quality, a real interview's
95% accuracy / 150ms timing targets, multicamera clap alignment, playback under
slow-disk or large-project load, Resolve/Premiere/Final Cut import, desktop
installers, general Astro/Next/WordPress management, host publishing and rollback.
