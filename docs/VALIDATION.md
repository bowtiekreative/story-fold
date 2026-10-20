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


## Recording responsiveness follow-up — 2026-10-20

The existing Dioxus studio now has explicit Play/Pause and Expand controls above
its native video surface. Pointer-hit tests verified play, pause, enter/exit and
Escape without reloading or replacing the playing video. Generated source clock
tests under a competing1080p encode measured314ms median/550ms p95 before the
buffer changes and173ms median/258ms p95 in the final shared-machine run after.
These figures exclude physical sensor/USB and browser painting. The same work
fixed a lost FFmpeg stdin control pipe: normal Stop now completed in281–390ms
instead of waiting for the8-second forced-kill fallback.58 focused unit tests,
strict lint and complete generated capture/preview/export checks passed;31
existing test-store files stayed byte-identical. Release service was rebuilt and
restarted only after confirming zero active recordings/exports; health passed.
See [the detailed proof](validation/recording-preview-responsiveness.json).

The separate egui studio now queues camera stop and take saving when Preview or
Export is selected during capture, with visible status instead of a disabled
control. An actual egui pointer test passed through save/export/player startup.
Its sequence player uses the installed mpv native controller with always-visible
Play/Pause, seek and fullscreen buttons. Controller mouse-binding tests through local mpv IPC verified
Play/Pause, a stopped playback clock, fullscreen enter/exit, Escape and replay
from the end, preserving the original MP4. A compiled-in local handler adds EOF
replay; arbitrary scripts/configs, sidecars and network extraction stay disabled.
The native generated filter-clock-to-RGBA benchmark measured109ms median/132ms
p95 across24 samples under a second1080p encode. This excludes camera sensor,
USB/driver and GPU painting; it does not establish zero sensor-to-screen delay.
See [native control evidence](validation/recording-native-buttons.json).

The initial M1–8 native release was packaged, extracted and started on this
Ubuntu24.04 host. A user launcher and icon were installed separately from the
interview editor. This is package extraction/startup acceptance, not a fresh-OS
installation test. [Package evidence](validation/record-studio-package.json).
Physical Logitech/OBSBOT, ten-minute A/V drift and Windows acceptance remain.


Native0.1.1 was rebuilt without test features, packaged and started from an
extracted.deb on an isolated display for8seconds with no stderr or missing
libraries. The user-installed binary and bundled downloader match the package
SHA256 values. Launcher validation and installer fixtures passed; the updated
window was launched as “Storyfold · Record studio (0.1.1)”. Older user windows
were preserved. Google/hardware/Windows and fresh-OS acceptance remain pending.
[Release evidence](validation/record-studio-0.1.1.json);
[YouTube evidence](validation/record-native-youtube.json).


The0.1.1 user retest reports that the whole native studio ignores clicks.
Interactive desktop acceptance has therefore failed and remains open. The egui
checks inject events directly into its context; the player test uses mpv local
IPC mouse/button commands to exercise its controller. Neither establishes native
window-system input delivery. A bounded opt-in native input/focus/modal/pending-
picker probe is being prepared; no hardware or private-content capture is needed.


## Native input follow-up and startup flow · 2026-10-20

A real X11/XTest fixture, restricted to a fresh Xvfb display, now finds this
app's own window and injects OS pointer events at its measured Project control.
The app receives them and opens the menu. This strengthens the earlier direct
egui-event and mpv-IPC checks; it still does not establish actual-desktop input.

Version0.1.2 adds visible Create/Open project actions above Record in both
supported shapes/minimum size, a grey inactive Record control, and native-parent
project folder dialogs.23 focused studio tests, strict all-target/all-feature
Clippy, the real OS-input fixture, installer checks and normal release build
passed. The package starts for8seconds after extraction with no stderr or missing
libraries; the installed binary hash matches it.
[0.1.2 release proof](validation/record-studio-0.1.2.json).

A separate opt-in, validation-feature-only Input check window records at most100
lines of event counts and busy/modal/focus flags. It records no key contents,
private paths, screenshots, credentials or media. No camera was started. The
initial desktop probe received no pointer events before its unused window was
replaced with the0.1.2probe; its absence of events is not evidence of the cause.
Ryan's Project-menu result is pending. This input repair stays open. Native folder
picker foreground behavior and physical camera timing remain unverified.
