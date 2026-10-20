# Decisions

- 2026-10-15: Use the supplied Storyfold and Catalyst Solo handoffs. Preserve the
  original prototypes as references. Ponytail full mode is active.
- Start with a local browser app and one local service rather than an installer.
  This makes the editor and build controls immediately reviewable on Linux.
- Catalyst manages Storyfold in this repository. It is not a separate general
  website builder. Port the useful review, task, preview, file and Git flows;
  do not embed the simulated prototype.
- Use an in-process job queue and local JSON storage for the single-user v1.
- No sample interview was supplied. Provide a clearly labeled synthetic media
  fixture and timed-word import. Do not claim speech accuracy from that fixture.
- Keep provider credentials in environment variables. Show readiness only, never
  their values. Do not make provider calls while rendering the synthetic sample.
- Git auto-push and publishing are off for this development build. Local review
  commits provide recovery without publishing incomplete work to the remote.
- Pause/cancel controls apply to local pipeline checks. They cannot suspend the
  Codex runtime; the UI must explain that distinction.
- Use an FFT library for audio-envelope correlation. A coarse sampled search
  produced a false match in the known-offset test; full normalized FFT
  correlation resolves the offset to 10ms while remaining fast on five minutes.
- Quantize video cuts by absolute 25fps boundary differences to prevent rounding
  drift across many concatenated clips. Audio retains exact sample timing.
- The first renderer uses 5ms fades at audio cut edges. A 10ms crossfade that
  preserves the exact shared story duration remains a later refinement.
- Doberman Core was added at Ryan's request. Package installation is verified;
  Ryan then explicitly authorized all Codex projects and future sessions. The
  user-wide hook is installed and default policy/configuration checks pass.
  The IDE's bundled Codex 0.162 is supported; the separate snap 0.114 is not.
  Host trust in a new session remains a required user action.

## Record section specification — 2026-10-19

- Ryan’s new recording brief applies to Storyfold Record; retain Storyfold
  branding and existing interview data. Its original text and requirement/status
  mapping are in `docs/reference/recording-studio-build-brief.txt` and
  `docs/RECORDING-SPEC.md`.
- Finish the reported live-preview and sequence-action repairs in the installed
  Rust/Dioxus app while building the confirmed egui recording architecture.
  This repair is not the complete segmented-capture/overlay-renderer migration.
- New takes negotiate supported Linux HD modes instead of accepting camera
  defaults. Continuous 720p30 preview replaces 640px snapshot polling; sources
  and exports support 1080p30. Existing low-resolution sources are preserved.
- Preview and MP4 export automatically save/prepare the current sequence, reuse
  only a current completed revision and invalidate previews after edits.
- The supplied reference originally required a paid AI budget STOP. Ryan later
  chose local AI (see the confirmed decision below); no paid requests or paid
  fallback are enabled.
- The new spec’s keychain storage, Unlisted default, per-project Git and shared
  native renderer are pending work, not properties of the current implementation.
- Ryan explicitly removed Doberman. That later instruction supersedes its old
  installation decision and the new reference’s historical Doberman sentence.

- Ryan subsequently confirmed the specified egui/eframe Record studio. Add
  `apps/sidebar-video` and `packages/video-core` alongside Catalyst Solo’s members;
  preserve its existing control panel code paths. Keep the current recording
  repair usable until the new native studio has feature parity.
- Ryan chose local AI for the Record overlay builder. This supersedes the
  reference’s paid-provider choice and monthly paid API budget STOP. No paid
  fallback is enabled. Verify local runtime/model and a real search source at
  the AI milestone; do not fabricate web results from a local language model.
