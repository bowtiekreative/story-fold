# Local development security

Bind only to loopback. Do not expose this service through a public tunnel: a user
who controls it can change source files and run the fixed build/test commands.
Validate Host and Origin and require the session token on mutations. Do not
enable CORS. Secrets live in the process environment or an ignored `.env` file.

Allow only known text source folders in Catalyst. Reject absolute paths, dot
segments, symlinks, `.git`, `.env`, credentials, media/data, dependency folders
and oversized files. Recheck file hashes before applying reviewed edits. Never
stage unrelated files; never use reset/clean/force push for recovery.

Uploaded media is treated as untrusted: generated filenames, a fixed extension,
ffprobe validation, 1–3 cameras, 2-hour duration limit, upload size limit, ffmpeg
argument arrays (no shell interpolation), fixed export artifact names and
project ID validation. The local OS must provide updated ffmpeg/ffprobe.

Local Whisper runs in an isolated Python environment with a downloaded model.
The setup command installs pinned faster-whisper from PyPI and downloads the
public small model from Systran on Hugging Face. Runtime model loading is
local-only with Hugging Face offline mode; interview audio stays local.
If local Whisper is unavailable and an OpenAI key is configured, the displayed
OpenAI Whisper fallback sends camera A audio to OpenAI. Topic classification
sends transcript text to the configured provider unless manual topics is
selected. Imported timed transcripts skip speech recognition; the synthetic
sample and manual-topic imports make no provider calls.

Single-user local use is the v1 boundary. Authentication, quotas, resource
isolation, encryption and upload scanning need a separate review before public
or multi-user deployment.
