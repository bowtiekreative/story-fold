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

## New recording specification — 2026-10-19

The [Record specification](RECORDING-SPEC.md) requires keychain-only provider keys
and OAuth tokens, explicit timed/budgeted network actions, reviewed AI proposals,
confined atomic project writes including hard-link rejection, and hardened local
project Git commits with credential scans. These are acceptance requirements for
the new implementation. The existing environment/private-file credential storage
is not keychain storage and must be migrated before claiming that milestone.
Do not silently submit camera frames/screenshots to providers or publish videos.
Keep existing localhost Host/Origin/token guards during the transition. Doberman
was removed at Ryan’s instruction and is not reinstalled by this reference.

Ryan confirmed local AI for Record. The recording overlay builder must keep
screenshots/prompts local and must not fall back to paid/cloud providers. Any
web search or page screenshot still requires an explicit user action; local AI
is not evidence that a page exists or a license to fetch arbitrary private URLs.
