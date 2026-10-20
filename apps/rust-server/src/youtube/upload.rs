use super::*;
use reqwest::StatusCode;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

const CHUNK: usize = 8 * 1024 * 1024; // Multiple of Google's required 256 KiB.
const INIT: &str = "https://www.googleapis.com/upload/youtube/v3/videos";
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    session: Option<String>,
    channel_id: String,
    sha256: String,
    total: u64,
    #[serde(default)]
    initiation_started: bool,
}
pub(super) fn validate_input(i: &UploadInput) -> Result<()> {
    identifier(&i.recording_id)?;
    identifier(&i.export_id)?;
    if !storyfold_core::identifier(&i.channel_id, 100) {
        return Err(AppError::bad(
            "Review and select a valid YouTube channel before uploading.",
        ));
    }
    if i.title.trim().is_empty()
        || i.title.chars().count() > 100
        || i.title.contains(['<', '>'])
        || i.title.chars().any(char::is_control)
        || i.description.len() > 5000
        || i.description.contains(['<', '>'])
        || i.description
            .chars()
            .any(|c| c.is_control() && !['\n', '\r', '\t'].contains(&c))
        || !["private", "unlisted", "public"].contains(&i.privacy.as_str())
    {
        return Err(AppError::bad(
            "Use a title of 1–100 characters, a description under 5,000 bytes without angle brackets, and Private, Unlisted, or Public visibility.",
        ));
    }
    Ok(())
}
fn metadata(i: &UploadInput) -> Value {
    json!({"snippet":{"title":i.title.trim(),"description":i.description,"categoryId":"22"},"status":{"privacyStatus":i.privacy,"selfDeclaredMadeForKids":i.made_for_kids}})
}
fn session_url(text: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(text)
        .map_err(|_| AppError::bad("YouTube returned an invalid upload session."))?;
    if url.scheme() != "https"
        || url.host_str() != Some("www.googleapis.com")
        || url.port_or_known_default() != Some(443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != "/upload/youtube/v3/videos"
        || url.query_pairs().find(|(k, _)| k == "upload_id").is_none()
    {
        return Err(AppError::bad(
            "YouTube returned an unexpected upload endpoint. No file was sent.",
        ));
    }
    Ok(url)
}
async fn hash(file: &std::path::Path, cancel: &CancellationToken) -> Result<(String, u64)> {
    let mut file = tokio::fs::File::open(file).await?;
    let mut digest = Sha256::new();
    let mut total = 0;
    let mut buf = vec![0; 1024 * 1024];
    loop {
        cancelled(cancel)?;
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 4 * 1024 * 1024 * 1024 {
            return Err(AppError::bad("YouTube uploads are limited to 4 GiB."));
        }
        digest.update(&buf[..n]);
    }
    if total == 0 {
        return Err(AppError::bad(
            "The export is empty. Export the recording again.",
        ));
    }
    Ok((format!("{:x}", digest.finalize()), total))
}
async fn snapshot(
    s: &AppState,
    j: &mut SavedJob,
    input: &UploadInput,
    cancel: &CancellationToken,
) -> Result<PathBuf> {
    let snapshot = asset(&s.store, &j.view.id, "upload.mp4")?;
    if let Some(cp) = &j.upload {
        if !snapshot.is_file() {
            return Err(AppError::bad(
                "The upload snapshot is missing. Check YouTube Studio before starting a new upload.",
            ));
        }
        let (sum, total) = hash(&snapshot, cancel).await?;
        if sum != cp.sha256 || total != cp.total {
            return Err(AppError::conflict(
                "The upload snapshot changed. Check YouTube Studio before creating a new upload.",
            ));
        }
        return Ok(snapshot);
    }
    j.view.stage = "Preparing an immutable export snapshot".into();
    save(s, j).await?;
    let source = recordings::exported_path(&s.store, &input.recording_id, &input.export_id)?;
    let before = tokio::fs::metadata(&source).await?;
    let temp = asset(&s.store, &j.view.id, "upload.pending")?;
    let mut options = tokio::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut output = options.open(&temp).await?;
    let mut source_file = tokio::fs::File::open(&source).await?;
    let mut buf = vec![0; 1024 * 1024];
    let mut total = 0;
    loop {
        cancelled(cancel)?;
        let n = source_file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > 4 * 1024 * 1024 * 1024 {
            return Err(AppError::bad("YouTube uploads are limited to 4 GiB."));
        }
        output.write_all(&buf[..n]).await?;
    }
    output.sync_all().await?;
    drop(output);
    let after = tokio::fs::metadata(&source).await?;
    if before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
        || total != before.len()
    {
        return Err(AppError::conflict(
            "The selected export changed while preparing it. Export again before uploading.",
        ));
    }
    tokio::fs::rename(&temp, &snapshot).await?;
    let (sha256, total) = hash(&snapshot, cancel).await?;
    let (_, channel) = auth::access(s).await?;
    if channel.id != input.channel_id {
        return Err(AppError::conflict(
            "The connected YouTube channel changed while preparing the export. Reconnect the reviewed channel and retry.",
        ));
    }
    j.upload = Some(Checkpoint {
        session: None,
        channel_id: channel.id,
        sha256,
        total,
        initiation_started: false,
    });
    j.view.bytes_total = total;
    save(s, j).await?;
    Ok(snapshot)
}
async fn account(s: &AppState, j: &SavedJob) -> Result<String> {
    let (token, channel) = auth::access(s).await?;
    if j.upload
        .as_ref()
        .is_some_and(|cp| cp.channel_id != channel.id)
    {
        return Err(AppError::conflict(
            "Reconnect the original YouTube channel to resume this upload.",
        ));
    }
    Ok(token)
}
pub(super) async fn run(
    s: &AppState,
    j: &mut SavedJob,
    i: &UploadInput,
    cancel: &CancellationToken,
) -> Result<()> {
    validate_input(i)?;
    // The final response was checkpointed before status became done. A crash in
    // that last local write cannot turn a confirmed upload into another transfer.
    if let Some(id) = &j.view.video_id
        && download::valid_video_id(id)
        && j.view.url.as_deref() == Some(format!("https://www.youtube.com/watch?v={id}").as_str())
    {
        return Ok(());
    }
    let snapshot = snapshot(s, j, i, cancel).await?;
    let client = client()?;
    if j.upload.as_ref().unwrap().session.is_none() {
        if j.upload.as_ref().unwrap().initiation_started {
            return Err(AppError::bad(
                "The upload-session request had an uncertain result. No media bytes were sent. Start a new upload after checking YouTube Studio.",
            ));
        }
        let access = account(s, j).await?;
        j.upload.as_mut().unwrap().initiation_started = true;
        j.view.stage = "Starting YouTube upload session".into();
        save(s, j).await?;
        let response = tokio::select! {_=cancel.cancelled()=>return Err(AppError::bad("Cancelled.")),r=client.post(INIT).query(&[("uploadType","resumable"),("part","snippet,status")]).bearer_auth(access).header("X-Upload-Content-Length",j.view.bytes_total).header("X-Upload-Content-Type","video/mp4").json(&metadata(i)).send()=>r?};
        if !response.status().is_success() {
            let status = response.status();
            j.upload.as_mut().unwrap().initiation_started = false;
            save(s, j).await?;
            return Err(api_error(status));
        }
        let location=response.headers().get("location").and_then(|v|v.to_str().ok()).ok_or_else(||AppError::bad("YouTube did not return an upload session. Start a new upload; no media bytes were sent."))?;
        let location = session_url(location)?.to_string();
        j.upload.as_mut().unwrap().session = Some(location);
        save(s, j).await?;
    }
    let session = session_url(j.upload.as_ref().unwrap().session.as_ref().unwrap())?.to_string();
    let total = j.upload.as_ref().unwrap().total;
    let mut offset = 0;
    let mut failures = 0;
    let mut query = true;
    let mut file = tokio::fs::File::open(snapshot).await?;
    loop {
        cancelled(cancel)?;
        let access = tokio::select! {_=cancel.cancelled()=>return Err(AppError::bad("Cancelled.")),r=account(s,j)=>r?};
        let chunk = if query {
            None
        } else {
            if offset >= total {
                return Err(AppError::bad(
                    "YouTube has all bytes but has not confirmed the video. Retry to check the same upload session.",
                ));
            }
            file.seek(std::io::SeekFrom::Start(offset)).await?;
            let mut bytes = vec![0; (total - offset).min(CHUNK as u64) as usize];
            file.read_exact(&mut bytes).await?;
            Some(bytes)
        };
        j.view.stage = if query {
            "Checking resumable upload"
        } else {
            "Uploading to YouTube"
        }
        .into();
        save(s, j).await?;
        let response = tokio::select! {_=cancel.cancelled()=>return Err(AppError::bad("Cancelled.")),r=send(&client,&session,&access,total,offset,chunk)=>r};
        match response {
            Ok(Reply::Done(id)) => {
                j.view.video_id = Some(id.clone());
                j.view.url = Some(format!("https://www.youtube.com/watch?v={id}"));
                j.view.bytes_sent = total;
                save(s, j).await?;
                return Ok(());
            }
            Ok(Reply::Resume(next)) => {
                if next > total {
                    return Err(AppError::bad("YouTube returned an invalid upload offset."));
                }
                if !query && next <= offset {
                    failures += 1;
                } else {
                    failures = 0;
                }
                offset = next;
                j.view.bytes_sent = offset;
                j.view.progress = (offset as f64 / total as f64 * 99.).min(99.);
                save(s, j).await?;
                query = false;
                if failures > 5 {
                    return Err(AppError::bad(
                        "YouTube stopped accepting upload bytes. Retry to check the same saved session.",
                    ));
                }
            }
            Ok(Reply::Unauthorized) => {
                auth::invalidate_access(s).await?;
                failures += 1;
                query = true;
            }
            Ok(Reply::Expired) => {
                return Err(AppError::bad(
                    "This YouTube upload session expired. Check YouTube Studio for the video before explicitly starting a new upload; Storyfold will not create a duplicate automatically.",
                ));
            }
            Ok(Reply::Fatal(status)) => return Err(api_error(status)),
            retry => {
                failures += 1;
                query = true;
                let retry_after = if let Ok(Reply::Retry(seconds)) = retry {
                    seconds
                } else {
                    None
                };
                if failures > 5 {
                    return Err(AppError::bad(
                        "The connection to YouTube could not recover. Retry to resume the same saved upload.",
                    ));
                }
                let seconds = retry_after.unwrap_or(1_u64 << failures.min(5)).clamp(1, 60);
                j.view.stage = "Connection interrupted; retrying saved session".into();
                save(s, j).await?;
                tokio::select! {_=cancel.cancelled()=>return Err(AppError::bad("Cancelled.")),_=tokio::time::sleep(Duration::from_secs(seconds))=>{}}
            }
        }
        if failures > 5 {
            return Err(AppError::bad(
                "Google authorization could not recover. Reconnect your channel and retry.",
            ));
        }
    }
}
#[derive(Debug, PartialEq)]
enum Reply {
    Resume(u64),
    Done(String),
    Unauthorized,
    Expired,
    Retry(Option<u64>),
    Fatal(StatusCode),
}
fn resume_range(headers: &reqwest::header::HeaderMap) -> Result<u64> {
    let Some(value) = headers.get("range") else {
        return Ok(0);
    };
    let value = value
        .to_str()
        .map_err(|_| AppError::bad("YouTube returned an invalid upload range."))?;
    value
        .strip_prefix("bytes=0-")
        .and_then(|s| s.parse::<u64>().ok())
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| AppError::bad("YouTube returned an invalid upload range."))
}
async fn send(
    client: &reqwest::Client,
    session: &str,
    access: &str,
    total: u64,
    offset: u64,
    chunk: Option<Vec<u8>>,
) -> Result<Reply> {
    let mut request = client
        .put(session)
        .bearer_auth(access)
        .header("Content-Type", "video/mp4");
    if let Some(bytes) = chunk {
        let n = bytes.len() as u64;
        if n == 0 || offset + n > total {
            return Err(AppError::bad("Invalid upload chunk."));
        }
        request = request
            .header(
                "Content-Range",
                format!("bytes {offset}-{}/{total}", offset + n - 1),
            )
            .header("Content-Length", n)
            .body(bytes);
    } else {
        request = request
            .header("Content-Range", format!("bytes */{total}"))
            .header("Content-Length", 0);
    }
    let response = request.send().await?;
    let status = response.status();
    if status == StatusCode::PERMANENT_REDIRECT {
        return Ok(Reply::Resume(resume_range(response.headers())?));
    }
    if status == StatusCode::UNAUTHORIZED {
        return Ok(Reply::Unauthorized);
    }
    if [StatusCode::NOT_FOUND, StatusCode::GONE].contains(&status) {
        return Ok(Reply::Expired);
    }
    if status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS {
        return Ok(Reply::Retry(
            response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse().ok()),
        ));
    }
    if !status.is_success() {
        return Ok(Reply::Fatal(status));
    }
    let data = bounded_json(response).await?;
    let id=data["id"].as_str().filter(|s|download::valid_video_id(s)).ok_or_else(||AppError::bad("YouTube accepted the upload but did not return a video identifier. Retry to check the same session."))?;
    Ok(Reply::Done(id.into()))
}
fn api_error(status: StatusCode) -> AppError {
    AppError::bad(match status {
        StatusCode::UNAUTHORIZED => {
            "Google sign-in expired. Reconnect your YouTube channel and retry."
        }
        StatusCode::FORBIDDEN => {
            "YouTube refused this upload. Check API quota, upload permission, verification, and channel restrictions in Google Cloud and YouTube Studio."
        }
        StatusCode::BAD_REQUEST => {
            "YouTube rejected the upload details. Check the title, description, visibility, and audience choice."
        }
        _ => {
            "YouTube could not finish this upload request. Retry the existing job to preserve its upload checkpoint."
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_defaults_and_session_hosts_are_safe() {
        let i: UploadInput = serde_json::from_value(
            json!({"recordingId":"studio","exportId":"export","channelId":"UC_reviewed","title":"Check","madeForKids":false}),
        )
        .unwrap();
        validate_input(&i).unwrap();
        assert_eq!(metadata(&i)["status"]["privacyStatus"], "private");
        assert_eq!(metadata(&i)["status"]["selfDeclaredMadeForKids"], false);
        for url in [
            "http://www.googleapis.com/upload/youtube/v3/videos?upload_id=x",
            "https://evil.test/upload/youtube/v3/videos?upload_id=x",
            "https://www.googleapis.com.evil.test/upload/youtube/v3/videos?upload_id=x",
            "https://user@www.googleapis.com/upload/youtube/v3/videos?upload_id=x",
            "https://www.googleapis.com/upload/youtube/v3/videos",
        ] {
            assert!(session_url(url).is_err());
        }
        assert!(session_url("https://www.googleapis.com/upload/youtube/v3/videos?upload_id=x&uploadType=resumable").is_ok());
        let mut headers = reqwest::header::HeaderMap::new();
        assert_eq!(resume_range(&headers).unwrap(), 0);
        headers.insert("range", "bytes=0-8388607".parse().unwrap());
        assert_eq!(resume_range(&headers).unwrap(), CHUNK as u64);
        headers.insert("range", "bytes=3-100".parse().unwrap());
        assert!(resume_range(&headers).is_err());
    }
    #[tokio::test]
    async fn mock_resumable_transport_checks_acknowledgement_and_completion() {
        use axum::{body::Bytes, http::HeaderMap};
        #[derive(Clone, Default)]
        struct Received(Arc<Mutex<Vec<(String, usize)>>>);
        async fn put(State(r): State<Received>, headers: HeaderMap, body: Bytes) -> Response {
            assert_eq!(headers["authorization"], "Bearer fake-test-token");
            let range = headers["content-range"].to_str().unwrap().to_string();
            r.0.lock().await.push((range.clone(), body.len()));
            if range.starts_with("bytes */") {
                (
                    StatusCode::PERMANENT_REDIRECT,
                    [("range", "bytes=0-262143")],
                    "",
                )
                    .into_response()
            } else {
                Json(json!({"id":"BaW_jenozKc"})).into_response()
            }
        }
        use axum::response::{IntoResponse, Response};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let received = Received::default();
        let app = Router::new()
            .route("/session", axum::routing::put(put))
            .with_state(received.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let url = format!("http://{address}/session");
        assert_eq!(
            send(&client().unwrap(), &url, "fake-test-token", 524288, 0, None)
                .await
                .unwrap(),
            Reply::Resume(262144)
        );
        assert_eq!(
            send(
                &client().unwrap(),
                &url,
                "fake-test-token",
                524288,
                262144,
                Some(vec![42; 262144])
            )
            .await
            .unwrap(),
            Reply::Done("BaW_jenozKc".into())
        );
        assert_eq!(
            *received.0.lock().await,
            vec![
                ("bytes */524288".into(), 0),
                ("bytes 262144-524287/524288".into(), 262144)
            ]
        );
        server.abort();
    }
}
