# Recording studio

For the native window titled **Storyfold · Record studio**, see the
[native studio guide](../catalyst-solo/boilerplate/README.md#storyfold-record-studio--in-development).
It includes microphone selection, clip waveforms and the native sequence player.
On Linux, **Preview sequence** plays the current cuts, audio, cards and media
inserts directly, without rendering an MP4 or saving a project version first.
It opens a snapshot of the current edits; press Preview again after changing them.
An active recording must finish saving its take before playback starts.
The tab-based instructions below describe the original Storyfold recorder.

The native Rust application's **Record** tab contains a separate studio for new
videos. Existing interviews remain in **Interviews**. Each recording saves its
own immutable source takes, editable sequence, text cards, and export metadata
under the local Storyfold data folder.

1. Choose **New recording**, name it, select horizontal or vertical, and choose
   a detected camera and microphone. The first microphone is selected initially;
   choose **Off** to record without opening a microphone. Video and microphone capture start only when
   you start a take. The studio checks for connected cameras every three seconds
   while idle and on returning to Record. Refresh preserves a valid camera
   selection and Microphone Off. On Linux, device capability queries exclude
   metadata-only nodes and do not start a video stream.
2. Hold the round button for a short take and release to save. A quick tap starts
   a long take; tap again or use **Stop and save** to finish. Separate start/stop
   buttons and keyboard controls are available. Stop is required before leaving
   the studio or closing the application.
3. Source takes appear in the bin. Edit their sequence using trim, split,
   move, remove, and add-again controls. These operations preserve source files.
   **Save changes** persists edits; **Reload saved** discards only the current
   studio draft. Unsaved drafts survive navigation to other app sections.
4. Use up to nine slots (keys **1–9**) for manual lower thirds, quotes, lists, or statistic cards. Choose **Show**
   while recording and **Hide** when finished. Saved source-time marks follow
   trims and repeated clips. Cards are text-based in this release. Review the
   rendered preview to see the final sequence and burned-in cards.
5. **Preview sequence** and **Export MP4** are available beside the viewer and
   your sequence. Each action saves your latest edits and prepares the current
   trimmed order and timed cards automatically. Preview opens the finished
   sequence in the viewer; Export MP4 asks where to save it. Both reuse an
   already prepared current version. MP4 output is 1080p H264/AAC at 30 fps,
   horizontal or vertical. Stop an active take before previewing or exporting.
   **Send to transcript editor** copies the finished sequence into an interview project,
   transcribes it with Whisper, and optionally groups its topics with Claude.
   A microphone-enabled take is needed for transcription.

Storyfold starts on **New project**. Open **Projects** to see saved interviews
and recordings, then explicitly open the one you want. Entering Record starts
with a new-project choice; it does not select an older recording automatically.
Switching tabs keeps an unsaved draft. Replacing a draft requires confirmation.

Preview shows a loading message until the finished video has decoded. If loading
fails, its error and **Retry loading** remain visible. **Expand preview** enlarges
the same player; use **Escape** to return. Export shows a loader immediately,
then the current rendering stage and percentage, followed by MP4 download
progress. The percentage comes from encoded time and transferred bytes.

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
FFmpeg checks cover capture, continuous HD preview, overlays, stop/finalize, portrait
rendering, interrupted-take recovery, and normalized imported media. Native tests
use only an explicit debug synthetic source and refuse physical cameras.

The connected Logitech C922 and OBSBOT Tiny 2 advertise 1080p at 30 fps using
MJPEG. Storyfold queries supported Linux frame intervals and prefers fluid HD
(up to 1080p30) over slow uncompressed HD modes. Drivers without discrete mode
enumeration retain their default mode. Source capture preserves up to 1080p;
existing lower-resolution takes cannot gain missing detail. The live viewer
receives a continuous, latest-frame MJPEG stream up to 1280×720 at 30 fps,
without reloading snapshots or accumulating a delayed frame queue. Actual
hardware capture quality and lip-sync still require a recording check.

Linux uses V4L2
and the default PulseAudio microphone; Windows uses DirectShow; macOS uses
AVFoundation. Operating system device permissions still apply. Google consent
and a real upload require the user's Desktop OAuth client and connected channel.

Linux discovery uses the per-node `device_caps` reported by
[VIDIOC_QUERYCAP](https://docs.kernel.org/userspace-api/media/v4l/vidioc-querycap.html),
rather than a physical device’s combined capabilities. This keeps USB metadata
endpoints out of the camera selector.

Capture mode selection follows the Linux
[frame interval query](https://docs.kernel.org/userspace-api/media/v4l/vidioc-enum-frameintervals.html)
and [FFmpeg V4L2 input options](https://ffmpeg.org/ffmpeg-devices.html#video4linux2_002c-v4l2).
