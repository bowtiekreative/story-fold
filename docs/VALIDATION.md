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
