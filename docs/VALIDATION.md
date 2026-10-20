# Actual validation

## Record preview/export repair — 2026-10-19 (Edmonton)

Verified on Ryan’s Linux workstation:

- `scripts/cargo.sh test --workspace --locked --offline`: **59 passed**, three
  explicitly opt-in tests ignored in that command.
- `scripts/cargo.sh build --workspace --locked --offline`: passed.
- `scripts/cargo.sh clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed after the nine-slot change.
- Opt-in generated capture/preview/export/recovery test passed. The continuous
  preview measured 1280×720 and 30 fresh frames in a one-second interval after
  startup. The test also checked 1920×1080 source capture, portrait export,
  source-byte preservation and interrupted-take recovery.
- Query-only connected-camera test selected MJPEG 1920×1080 at 30 fps for OBSBOT
  Tiny 2 and Logitech C922. It did not start camera or microphone capture.
- Native Dioxus/WebKit test under Xvfb passed generated hold/tap capture,
  full-sequence playback, automatic save before first preview, cache reuse,
  stale-preview invalidation after trim, source preservation and draft navigation.
- The actual Export MP4 button saved a generated H264/AAC MP4, verified by
  ffprobe as 1920×1080 at 30 fps. The test supplied its temporary save path;
  manual operation of the OS save dialog was not checked.
- `scripts/cargo.sh build --workspace --release --locked --offline`: passed;
  the rebuilt service started healthy and hashes of 29 saved project/recording
  files remained unchanged. Existing editor windows were preserved.
- Nine-slot UI and runtime-limit regressions passed; native controls remain
  separate from the forthcoming egui foundation.

Evidence: [native checks](validation/recording-preview-native.json) and
[media/quality checks](validation/recording-preview-quality.json).

These checks do not establish physical-camera smoothness, microphone lip-sync,
one-frame hold timing, ten-minute audio drift, unplug recovery, Windows behavior,
keychain migration, AI overlays, zoom inserts or clean-install acceptance. No
YouTube video was uploaded. The supplied new studio’s milestones retain those
acceptance checks separately in [RECORDING-SPEC.md](RECORDING-SPEC.md).

Earlier verification is retained in [validation/RESULTS.md](validation/RESULTS.md).

## Fresh startup, sequence playback and export feedback — 2026-10-19 (Edmonton)

- Locked offline Rust workspace tests: **68 passed**, three opt-in tests ignored.
  Strict all-target Clippy and the debug workspace build passed.
- The native application check on the actual Linux desktop passed fresh startup,
  the populated Projects tab, no implicit selection of older media, generated
  hold/tap recording, playback after Preview sequence, cache invalidation,
  Expand/Escape, immediate export loading, MP4 saving and draft preservation.
  The generated result is H264/AAC,1920×1080 at30fps,16.267seconds.
- A generated three-clip export reported real intermediate progress through
  individual clips and final mux, then100% only at completion. Download tests
  verify actual byte progress and preservation of an existing destination after
  interruption. The native harness supplies a temporary save destination;
  manual interaction with the OS save dialog remains unverified.
- A muted, read-only native decode of an existing local export reached ready
  state4 and decoded1920×1080 video. Its original source media hashes were
  preserved. This check did not rebuild the older export or inspect its content.
- A separate Xvfb run lost its WebKit connection during generated take
  finalization and timed out. The actual desktop run passed; headless recording
  acceptance is not claimed for this update. No GPU workaround was shipped.

Evidence: [native application](validation/recording-feedback-native.json),
[read-only playback](validation/recording-feedback-playback.json) and
[real export progress](validation/recording-feedback-progress.json).
The locked offline release build passed and the updated local service and
desktop window were launched. See [release checks](validation/recording-feedback-release.json)
for the saved-file preservation check.
Physical-camera smoothness, device timing/drift and the new egui studio's full
acceptance remain separate checks.

## egui Record foundation — 2026-10-19 (Edmonton)

Catalyst Solo commit `a084433` adds `apps/sidebar-video` and `packages/video-core`
with the pinned eframe0.33.0, AccessKit, Wayland/X11, bundled unmodified Inter400/600
and the SIL OFL. Existing Catalyst application/core/CLI code was unchanged.

The nested workspace passed formatting, all14 tests, warning-denied Clippy and
a locked offline build. The final layout adjustment passed its2 affected native
UI tests, lint and rebuild. Real Xvfb launches at1480×1000 horizontal and minimum
1160×800 portrait exited successfully; the coordinator inspected both screenshots.
Tests cover output dimensions, bounded stable slots, shared credential checks,
AccessKit labels/control bounds and text contrast.

This is a layout/core foundation; it opens no device, project or model and does
not yet record, edit clips, render, persist, infer or upload. Windows CI and
screen-reader operation were not checked. Capture/timeline/AI acceptance remain
tracked requirements. See the nested `docs/VALIDATION.md` for commands and proof.
