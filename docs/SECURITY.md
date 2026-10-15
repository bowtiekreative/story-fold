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

Real transcription sends audio to OpenAI only when explicitly selected on
upload. Topic classification sends transcript text to the configured provider.
The offline sample and imported timed transcripts do not call a provider.

Single-user local use is the v1 boundary. Authentication, quotas, resource
isolation, encryption and upload scanning need a separate review before public
or multi-user deployment.
