# Storyfold Record specification

Ryan supplied the [native studio build brief](reference/recording-studio-build-brief.txt)
on 2026-10-19 and asked that it govern Storyfold’s **Record** section, with
Storyfold branding retained. The supplied file is preserved verbatim as a
reference. This document maps its requirements to the existing application;
it does not assert that the full studio has been implemented.

## Confirmed decisions and remaining setup

- Ryan confirmed the specified **egui/eframe Record studio**. Build its two
  members in the Catalyst Solo Cargo workspace, keeping Catalyst’s existing
  code paths intact. The installed Dioxus recording repair remains usable until
  the new studio has feature parity; an empty foundation does not replace it.
- Ryan chose **local AI** for Record’s overlay builder. This supersedes the
  reference’s paid Claude/ChatGPT provider and paid API budget gate. No paid
  provider fallback. Local runtime/model and a real search source still need
  verification before the AI milestone; no provider calls occur in milestone 1.
- Live YouTube publishing requires Ryan’s Google Desktop OAuth client and
  consent. An app-side connection control or mocked upload is not that consent.

## Required behavior and implementation

Keep Storyfold’s name, visual identity and existing interviews. Apply the new
brief to Record; do not alter Catalyst Solo’s internal code paths. Reuse its
public credential scan, path confinement and private-directory helpers where
applicable. Ubuntu is the first supported recording platform; retain honest
Windows compile and runtime validation status.

- Hold Record or Space to mark a clip; a configurable tap threshold defaults to
  350 ms and tap-tap makes one long take. One ordered bin/sequence supports drag
  reorder and Move left/right, split at playhead, both-end frame/second trim
  nudges, Removed with files/cues retained, and 100 project-snapshot Undo/Redo.
- Nine overlay slots, keys 1–9: lower third, stat, screenshot card, numbered
  list, photo and quote. Cues follow clip IDs and millisecond offsets. Moving,
  splitting, trimming and deleting must preserve the brief’s cue invariants.
- Explicitly activate a Studio capture session. A long-running Linux FFmpeg
  sidecar produces one-second H264/AAC buffer segments and a small raw RGBA
  preview; button gestures mark intervals rather than reopening the camera.
  Keep the start frame within one frame of the press; test 20 holds, ten-minute
  audio drift under one frame and unplug recovery. Prune unused buffer after
  ten minutes. Other platforms need a compiling capture stub.
- One tiny-skia/cosmic-text renderer must draw preview and export overlays from
  the same time-dependent RGBA path. Photo/video zoom supports start/end boxes,
  100–250%, duration, Smooth/Steady/Snappy, and full/beside/behind placement.
  Compare preview/export at 0, 2.5 and 5 seconds.
- Horizontal 1920×1080 or vertical 1080×1920; shape may change outside capture.
  Export H264/AAC MP4 with cards burned in, actual progress and cancellation.
  A vertical video longer than 180 seconds gets the brief’s Shorts notice.
- AI search uses the selected provider’s real web search, bounded requests,
  Chromium screenshots and 4 result cards by default (setting 1–12). Accept
  dropped screenshots. Rebuild only as a validated strict proposal containing
  summary, kind, title, source_url, blocks and highlight offsets; copied text
  must match the screenshot. Review screenshot and rebuilt card side by side;
  approve/edit/reject before persistence. Approved cards can export standalone
  HTML. Use Ryan’s local AI runtime with an editable model name; no paid provider fallback.
- Network work runs only after explicit actions, with timeouts and UI channels.
  Verify the local runtime request format before implementing calls. Track
  local token usage. The paid API budget requirement is superseded by local AI.
- Keys and OAuth tokens belong in the OS keychain. Never write them into source,
  project JSON, logs or Git. The current runtime’s private environment and
  OAuth-file storage must not be represented as keychain compliance.
- New project folders contain project.json, overlays/, media/, buffer/,
  exports/ and .sidebar/private/. Initialize local Git only for new folders;
  track project/overlay JSON and ignore media, buffer, exports and private data.
  Save version and every export make a local commit; never auto-push. Use
  hardened app-owned Git settings, no UI shell strings, credential scans,
  confined atomic writes, and rejection of traversal, symlinks and hard links.
- Keep full keyboard operation, AccessKit semantics for the requested egui UI,
  44px targets, 4.5:1 text contrast, reduced motion, and licensed Inter 400/600.
  Storyfold branding supersedes the reference palette/name. Layout remains
  header/shape/export, ordered takes left, stage/88px Record center, overlay
  slots right, sequence and overlay timing track below.

## Milestones and actual status

| Step | Deliverable | Current status |
| --- | --- | --- |
| 1 | Workspace/window foundation; architecture, security, decisions | egui/eframe confirmed; native foundation in progress in Catalyst Solo. Existing Dioxus studio remains usable. |
| 2 | Core project/timeline model and every cue/save invariant test | Partial existing clip/take model; new cue model and full invariant suite pending. |
| 3 | Ordered bin/sequence, drag, playhead tools, shape, Removed, Undo | Existing move/split/trim/shape; remaining operations pending. |
| 4 | Persistent segmented capture and hardware acceptance | Current per-take capture has HD mode selection and streamed preview fixes; segmented session capture and hardware timing checks pending. |
| 5 | Shared renderer, six overlay types, nine slots and playback | Nine slots/keys and four text card types with ASS export are implemented; shared renderer/images/animation pending. |
| 6 | Search/screenshot/AI review, HTML, budget and usage | Pending; local AI confirmed, runtime/model and search source need verification. |
| 7 | Media inserts and matching zoom preview/export | Pending. |
| 8 | 1080p MP4 in both shapes, progress/cancel | HD render/preview/export repair in validation; cancellation and full specification acceptance pending. |
| 9 | Keychain OAuth/PKCE resumable upload, Unlisted default | Existing explicit Private upload flow is partial; keychain migration/new default pending. No live publishing acceptance claimed. |
| 10 | Ubuntu .deb, icon, clean-install acceptance | Existing local launcher; package and clean-install acceptance pending. |

Record actual commands, dates and remaining limitations in
[VALIDATION.md](VALIDATION.md). Keep the source brief’s acceptance conditions;
passing a generated smoke test does not prove physical-device timing or drift.
