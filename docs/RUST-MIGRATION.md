# Storyfold migration to Rust

Ryan requested this as the next build after the standalone Catalyst Solo Rust
starter is finished. Catalyst Solo is published and verified on Ubuntu and Windows. The migration
is implemented; the previous web application remains available for recovery.

## Sequence and completion gate

1. Finish Catalyst Solo: compile, format, lint and test its Rust workspace;
   validate the desktop flow; publish the complete starter and original references
   to `bowtiekreative/business-catalyst-solo`.
2. Start Storyfold's Rust migration after that gate. Implement the application
   core and native desktop interface in Rust. The existing web application can
   remain available during the transition.
3. Switch the default launcher only after feature parity and saved-project
   compatibility are verified. Keep the previous release available for recovery.

Catalyst Solo is published at
https://github.com/bowtiekreative/business-catalyst-solo (commit d38e3d2).
Its formatting, nine tests, Clippy, build and native startup passed locally, and
Ubuntu/Windows CI passed. Doberman was removed by Ryan and removal was verified.

The Storyfold implementation uses a shared Rust timeline/model crate, an Axum
runtime, and a Dioxus native desktop editor. The system WebView supplies video
and audio decoding. The isolated Python Whisper worker remains an explicit ML
adapter; Node and TypeScript are not required by the new application runtime.

## Existing behavior to preserve

- Upload one to three videos, probe media, create proxies and waveforms, and
  retain camera offsets and virtual crops. Camera A remains the audio master.
- Transcribe automatically with local Whisper and word timestamps. Preserve
  optional transcript import, explicitly identified provider fallback, chunk
  caching, interruption recovery and manual topic organization.
- Group long transcripts with Claude using one shared topic taxonomy and bounded
  assignment batches. Preserve strict sentence coverage, retry behavior, manual
  fallback and real progress. Do not restore the old 400-sentence restriction.
- Edit sentences, topic folders, trims, camera choices and L-cuts. Preview and
  export must use the same timeline calculations.
- Export MP4, SRT, EDL, FCPXML and chapters with revision snapshots. Preserve
  documented limitations until their acceptance checks actually pass.
- Open existing project JSON and reuse media, transcript and processing outputs
  in `data/projects/`, including a configured `STORYFOLD_DATA_DIR`. Preserve IDs,
  timestamp precision, ordering and revision-conflict protection.
- Keep build visibility, reviewed source changes, Git undo, task priorities,
  pause and cancel controls available through the Rust control panel.

## Implementation order

Port `packages/timeline` and its invariants first. Use synthetic compatibility
fixtures to compare the TypeScript and Rust outputs for splitting, sequence
ordering, audio/video timing, captions and interchange exports.

Next port storage, validation, serialized project writes and revision checks,
then media job orchestration, progress, cancellation, resume and provider calls.
FFmpeg/ffprobe remain external media tools. Qualify the Whisper backend against
existing word-timing and resume behavior before replacing the current worker;
any temporary Python bridge must be documented as transitional.

Build the Rust desktop editor around the tested core. Native video playback,
audio/video synchronization and accessibility require explicit validation;
reusing the Catalyst shell alone does not establish editor parity.

## Acceptance before switching

- Existing saved projects open without modifying or losing their source data.
  Exercise migration with copies and explicit backups before any schema write.
- A video-only upload produces timed words automatically and an editable story.
- A transcript over 400 sentences groups completely, and interrupted processing
  resumes from cached outputs without losing edits or timings.
- Trim, reorder, skip, topic and camera edits survive restart; stale revisions
  fail without overwriting newer work.
- Playback and rendered media preserve the existing L-cut timing checks;
  captions, chapters and interchange files match the selected story revision.
- Provider keys stay private. Local transcription remains local, and provider
  disclosures, path boundaries and mutation checks remain effective.
- Rust checks and native smoke tests pass on the supported platforms. Real
  interview accuracy, multicamera alignment and NLE import remain separate
  evidence requirements; do not imply they passed from synthetic fixtures.

This migration does not establish production acceptance for every media workflow
or platform. The outstanding checks below remain explicit.

## Verified integration evidence

- Rust timeline, sentence splitting, SRT, EDL and FCPXML match the generated
  TypeScript compatibility fixture. The native editor imports the same core
  model and timeline implementation used by export.
- Revision-race, backup, read-only load, path/symlink, media byte-range and reviewed
  Git edit/undo integration tests pass. Unrelated staged work is preserved.
- [Rust media API](validation/rust-media.json): uploaded generated footage,
  saved cuts and rendered a six-second portrait video. Picture changes at 2s;
  master audio changes at 3s. Captions, two chapters and interchange artifacts
  passed; XML was structurally parsed.
- [Local Whisper](validation/rust-whisper.json): a generated video-only upload
  produced 35 timed words and three sentences without imported transcript JSON.
- [Live Claude](validation/rust-claude.json): all 612 generated sentences were
  assigned to four shared topics across three bounded assignment batches.
- [Native Linux window](validation/rust-native.json): sample import, processing,
  sentence editing, save/readback, Undo/Redo, actual H.264/AAC playback and
  navigation between the editor and Build pane passed. The test restored its
  generated sample after editing. The app also handles GTK loader paths inherited
  from a Snap-hosted IDE without changing WebKit sandbox settings.
- [Optimized Linux release](validation/rust-release.json): both native binaries
  built, the desktop opened its bundled service, and the release API passed the
  full rendered-media check. Local Whisper and Claude reported ready. Loading
  an existing saved interview left its original JSON bytes unchanged.

The complete workspace passes formatting, all 28 tests and Clippy with warnings
denied. The retained web release also passes its Ubuntu/Windows checks and
browser/media acceptance suite. The isolated Python adapter remains necessary
for local Whisper; the application and orchestration no longer need Node.

[Rust CI](validation/rust-ci.json) passed tests, Clippy and optimized builds on
Ubuntu and Windows for source commit `ffe26a0`. Ubuntu also passed the real
HTTP/FFmpeg media check. Windows required a canonical fixture root and a
repository-relative private Git index; reviewed apply/undo and preservation of
unrelated staged work now pass there too.

[Build control](validation/rust-build-control.json) launched the complete Cargo
test suite from the running app and retained its passing output in check history.
One 15-second test polling request timed out during compilation; the job completed
and subsequent history readback succeeded in 0.08 seconds. This does not establish
performance on slow disks.

These synthetic fixtures do not establish real-footage
transcription accuracy, multicamera alignment or NLE import acceptance.
