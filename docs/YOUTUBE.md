# YouTube clips and publishing

Storyfold's Recording studio can import a selected YouTube interval into its clip bin and upload a finished recording export to your own channel. Importing and publishing are separate actions. Opening a studio, connecting an account, or exporting locally never publishes a video.

## Import a clip

1. Open or create a Recording studio.
2. Paste a single YouTube video URL and enter start/end times in seconds. Watch, youtu.be, Shorts and embed links are accepted; tracking and playlist parameters are removed before use. The interval must be 0.1 seconds to 10 minutes and within the first 24 hours of a finished video.
3. Confirm that you have permission to use the source and click Import. The app downloads only the selected interval, converts it to H264/AAC MP4, and appends a take to this studio's bin. Original recorded takes remain intact.
4. Jobs report progress and can be cancelled or retried. A completed normalized clip is retained locally, so retrying an interrupted append does not download it again or create another take.

One-time importer setup: `python3 scripts/setup-youtube.py`. This creates an isolated runtime in ignored `data/youtube-runtime`, installs the official `yt-dlp[default]` distribution from PyPI, and checks FFmpeg and Node.js 22+. Repeating the command explicitly updates the importer. The application prefers this runtime; an existing PATH yt-dlp is a fallback, but old versions may need this setup. It never reads a user's yt-dlp config or browser cookies, loads external plugins, disables certificate checks, or downloads remote challenge components at runtime. FFmpeg network inputs explicitly enforce `tls_verify=1`, including older builds whose default is disabled. The setup installs the official default challenge package; the installed Node runtime executes it.

Private, paid, unavailable, protected and live-stream sources can fail at YouTube. This integration does not obtain access to them. For footage you own, importing its authorized original file is the reliable alternative. Only use material you own or are permitted to reuse. YouTube can change its download behavior, so imports are best-effort and remain separate from the official publishing API.

## Connect your channel once

Google requires a registered OAuth client for a desktop application. No client or Google sign-in was fabricated during development.

1. In your Google Cloud project, enable **YouTube Data API v3**.
2. Configure Google Auth Platform branding, audience and data access. If the application is in Testing, add your own Google account as a test user. Google's review and testing restrictions apply to your project and account.
3. Create an OAuth client of type **Desktop app** and download its client JSON. A web client or API key does not replace it.
4. In the studio's YouTube controls, choose that JSON through the native file picker. The app accepts Google's standard installed-client fields and endpoints; it does not accept a server file path from HTTP input.
5. Click **Connect**. Storyfold opens your normal system browser. Sign in and grant YouTube upload plus read-only channel access. Select the channel you intend to use. Return to Storyfold and verify the displayed channel name before uploading.

Authorization uses a temporary random port on `127.0.0.1`, a unique state nonce, a SHA-256 PKCE challenge, one-use callback handling and a five-minute timeout. Google credentials are exchanged only with the standard HTTPS token endpoint. Redirects are not followed for authenticated API requests.

Local credentials live under ignored `data/youtube` (or the configured Storyfold data directory). On Unix, the directory is 0700 and atomically written credential files are 0600. Windows uses the account's inherited data-directory ACL; this is file-based local credential storage, not a portable encrypted vault. Tokens, refresh tokens, OAuth codes and upload session URLs are excluded from the public job/status API, user-visible logs, source commits and docs. Keep your local account/data folder private. **Disconnect** cancels jobs, deletes the local token file and attempts Google's token revocation. If revocation cannot be confirmed, the UI explains how to remove access in your Google account. The client setup remains for reconnecting.

## Review and upload

1. Export the current recording sequence. Editing the sequence makes that export stale for a new upload; export again after edits.
2. Check the connected channel, video title, description, visibility and audience choice. **Private** is Storyfold's explicit default. Public or Unlisted requires choosing that option. Google and YouTube account/project rules may further restrict an upload's visibility or processing.
3. Click **Upload to YouTube**. This is the user action that starts the external upload. Storyfold binds the request to the reviewed channel ID, snapshots the selected finished export and sends it using the official YouTube Data API. If another window changes the connected account, the queued job stops rather than using that new channel.
4. Progress is acknowledged bytes, sent in 8 MiB chunks. The app checks the same resumable session after a network failure, backs off and retries a bounded number of times, refreshes expired access tokens, and saves its session checkpoint after each acknowledgement. A final YouTube watch link appears after the API confirms the video identifier; YouTube processing can continue afterward.

Cancellation stops further transfer and retains its checkpoint. It does not claim to delete a video that YouTube may already have accepted. Retry after an application restart uses the same immutable snapshot, hash, target channel and upload session. If the session expires or the final result is uncertain, Storyfold asks you to check YouTube Studio before explicitly creating a new upload; it never automatically starts a duplicate. Reconnecting a different channel cannot resume an existing job targeting the original channel.

## Validation and limits

Automated checks exercise URL canonicalization/SSRF input rejection, finite bounds, live/playlist metadata rejection, exact clip normalization with generated video and silent-source audio synthesis, private defaults, PKCE, standard endpoint validation, private credential permissions, authenticated local mutations, interrupted-job identity and a mock HTTP resumable transfer with status query, byte ranges and final video response. These tests do not publish a real video or open a physical camera.

Live Google account consent and an actual upload require your Desktop OAuth client and browser approval. Those remain manual acceptance checks; a mocked response is not evidence of a connected account or a published video. Real public-video downloads also depend on YouTube availability, region/network access and current yt-dlp support. The download phase limits temporary work to 2 GiB and 30 minutes; uploads are limited to a 4 GiB local export. One external job runs at a time, with a queue and per-recording duplicate-start protection. Local snapshots and sources are retained for recovery; there is no automatic retention deletion policy in this release.

Primary implementation references: [Google desktop OAuth](https://developers.google.com/identity/protocols/oauth2/native-app), [YouTube resumable upload protocol](https://developers.google.com/youtube/data/guides/using_resumable_upload_protocol), [videos.insert](https://developers.google.com/youtube/data/reference/rest/v3/videos/insert), [FFmpeg TLS options](https://ffmpeg.org/ffmpeg-protocols.html#tls), and [official yt-dlp options](https://github.com/yt-dlp/yt-dlp#usage-and-options).
