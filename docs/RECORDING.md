# Recording studio

The native Rust application's **Record** tab contains a separate studio for new
videos. Existing interviews remain in **Interviews**. Each recording saves its
own immutable source takes, editable sequence, text cards, and export metadata
under the local Storyfold data folder.

1. Choose **New recording**, name it, select horizontal or vertical, and choose
   a detected camera and microphone. The first microphone is selected initially;
   choose **Off** to record without opening a microphone. Devices open only when
   you start a take. Refreshing the device list preserves a valid selection.
2. Hold the round button for a short take and release to save. A quick tap starts
   a long take; tap again or use **Stop and save** to finish. Separate start/stop
   buttons and keyboard controls are available. Stop is required before leaving
   the studio or closing the application.
3. Source takes appear in the bin. Edit their sequence using trim, split,
   move, remove, and add-again controls. These operations preserve source files.
   **Save changes** persists edits; **Reload saved** discards only the current
   studio draft. Unsaved drafts survive navigation to other app sections.
4. Add manual lower thirds, quotes, lists, or statistic cards. Choose **Show**
   while recording and **Hide** when finished. Saved source-time marks follow
   trims and repeated clips. Cards are text-based in this release. Review the
   rendered preview to see the final sequence and burned-in cards.
5. **Render recording** creates a 720p H264/AAC sequence in the selected orientation.
   **Save MP4** uses a native save dialog. Re-render after edits. **Send to
   transcript editor** copies the finished sequence into an interview project,
   transcribes it with Whisper, and optionally groups its topics with Claude.
   A microphone-enabled take is needed for transcription.

Interrupted takes are recovered from their local capture container when possible
and labeled for review. Stop waits for capture to finish before publishing a take
as complete. Saved revisions reject stale edits rather than overwriting newer
work. Recording is limited to two hours and 4 GiB per take. Source files and
exports are retained locally; there is no automatic cleanup policy.

YouTube URL imports and channel uploads are described in [YOUTUBE.md](YOUTUBE.md).
Exporting an MP4 or connecting Google does not publish anything. Upload requires
the explicit upload action, with Private selected by default.

## Verification

Rust tests cover gesture timing and delayed acknowledgements, draft preservation,
clip validation, source preservation, stale revisions, device/path boundaries,
overlay timing, local API authorization, and YouTube requests. Opt-in generated
FFmpeg checks cover capture, JPEG preview, overlays, stop/finalize, portrait
rendering, interrupted-take recovery, and normalized imported media. Native tests
use only an explicit debug synthetic source and refuse physical cameras.

No webcam is exposed on the development host, so real camera/microphone device
compatibility and lip-sync remain hardware acceptance checks. Linux uses V4L2
and the default PulseAudio microphone; Windows uses DirectShow; macOS uses
AVFoundation. Operating system device permissions still apply. Google consent
and a real upload require the user's Desktop OAuth client and connected channel.
