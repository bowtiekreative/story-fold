use super::*;
use serde_json::json;
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{io::AsyncReadExt, process::Command};

pub(super) fn valid_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
pub(super) fn canonical_url(text: &str) -> Result<String> {
    let fail = || AppError::bad("Paste a YouTube watch, Shorts, or youtu.be URL for one video.");
    if text.len() > 4096 {
        return Err(fail());
    }
    let u = reqwest::Url::parse(text.trim()).map_err(|_| fail())?;
    if !["https", "http"].contains(&u.scheme())
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some_and(|p| p != 443 && p != 80)
    {
        return Err(fail());
    }
    let host = u.host_str().ok_or_else(fail)?;
    let id = if ["youtu.be", "www.youtu.be"].contains(&host) {
        u.path()
            .strip_prefix('/')
            .filter(|p| !p.contains('/'))
            .ok_or_else(fail)?
            .to_string()
    } else if [
        "youtube.com",
        "www.youtube.com",
        "m.youtube.com",
        "music.youtube.com",
    ]
    .contains(&host)
    {
        if u.path() == "/watch" {
            let ids: Vec<_> = u.query_pairs().filter(|(k, _)| k == "v").collect();
            if ids.len() != 1 {
                return Err(fail());
            }
            ids[0].1.to_string()
        } else {
            let parts: Vec<_> = u.path().split('/').collect();
            if parts.len() != 3 || !["shorts", "embed", "live"].contains(&parts[1]) {
                return Err(fail());
            }
            parts[2].into()
        }
    } else {
        return Err(fail());
    };
    if !valid_video_id(&id) {
        return Err(fail());
    }
    // All playlist, timestamp, redirect and tracking parameters are discarded.
    Ok(format!("https://www.youtube.com/watch?v={id}"))
}
pub(super) fn validate_range(start: f64, end: f64) -> Result<()> {
    if !start.is_finite()
        || !end.is_finite()
        || start < 0.
        || end > 86400.
        || end - start < 0.1
        || end - start > 600.
    {
        return Err(AppError::bad(
            "Choose a clip between 0.1 seconds and 10 minutes within the video's first 24 hours.",
        ));
    }
    Ok(())
}
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}
fn command(store: &Store) -> Option<(PathBuf, Vec<OsString>)> {
    let runtime = store.data.join("youtube-runtime");
    if reject_link(&runtime).is_err() {
        return None;
    }
    let python = runtime.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    if python.is_file() && runtime.join("ready.json").is_file() {
        Some((python, vec!["-I".into(), "-m".into(), "yt_dlp".into()]))
    } else {
        on_path(if cfg!(windows) {
            "yt-dlp.exe"
        } else {
            "yt-dlp"
        })
        .map(|p| (p, Vec::new()))
    }
}
pub(super) fn available(store: &Store) -> bool {
    command(store).is_some()
}
fn defaults() -> Vec<OsString> {
    [
        "--ignore-config",
        "--no-plugin-dirs",
        "--no-remote-components",
        "--no-playlist",
        "--no-cache-dir",
        "--no-cookies",
        "--no-cookies-from-browser",
        "--no-mark-watched",
        "--no-warnings",
        "--no-colors",
        "--socket-timeout",
        "20",
        "--retries",
        "2",
        "--extractor-retries",
        "2",
        "--fragment-retries",
        "2",
        "--proxy",
        "",
        "--use-extractors",
        "youtube",
        "--downloader-args",
        "ffmpeg_i:-tls_verify 1",
        "--js-runtimes",
        "node",
    ]
    .into_iter()
    .map(Into::into)
    .collect()
}
fn info(value: &Value, url: &str, start: f64, end: f64) -> Result<String> {
    if value["id"]
        .as_str()
        .map(|id| format!("https://www.youtube.com/watch?v={id}"))
        .as_deref()
        != Some(url)
        || value["_type"].as_str().is_some_and(|s| s != "video")
        || value["is_live"].as_bool() == Some(true)
        || ["is_live", "is_upcoming", "post_live"]
            .contains(&value["live_status"].as_str().unwrap_or(""))
    {
        return Err(AppError::bad(
            "Choose one finished YouTube video; live streams and playlists cannot be imported.",
        ));
    }
    let duration = value["duration"]
        .as_f64()
        .filter(|n| n.is_finite() && *n > 0.)
        .ok_or_else(|| AppError::bad("YouTube did not provide a finite video duration."))?;
    if start >= duration || end > duration + 0.1 {
        return Err(AppError::bad(
            "The selected clip extends beyond the end of the YouTube video.",
        ));
    }
    Ok(value["title"]
        .as_str()
        .unwrap_or("YouTube clip")
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect())
}
pub(super) async fn run(
    s: &AppState,
    j: &mut SavedJob,
    i: &ImportInput,
    cancel: &CancellationToken,
) -> Result<()> {
    validate_range(i.start, i.end)?;
    let url = canonical_url(&i.url)?;
    let (bin, prefix) = command(&s.store).ok_or_else(|| {
        AppError::bad("Install the YouTube importer with python3 scripts/setup-youtube.py.")
    })?;
    let dir = directory(&s.store, &j.view.id)?;
    let normalized = asset(&s.store, &j.view.id, "clip.mp4")?;
    // The completed normalized source is itself a recoverable import checkpoint.
    let title = if normalized.is_file() {
        let info: Value = read_json(&asset(&s.store, &j.view.id, "source.json")?).await?;
        validate_normalized(&normalized, i.end - i.start, cancel).await?;
        info["title"].as_str().unwrap_or("YouTube clip").to_owned()
    } else {
        j.view.stage = "Reading YouTube video information".into();
        j.view.progress = 5.;
        save(s, j).await?;
        let mut args = prefix.clone();
        args.extend(defaults());
        args.extend([
            "--dump-single-json".into(),
            "--skip-download".into(),
            "--".into(),
            url.clone().into(),
        ]);
        let raw = tool(
            &bin,
            &args,
            &dir,
            cancel,
            Duration::from_secs(180),
            Some((s, j)),
        )
        .await?;
        let meta: Value = serde_json::from_slice(&raw).map_err(|_| {
            AppError::bad("YouTube metadata could not be read. Update the importer and try again.")
        })?;
        let title = info(&meta, &url, i.start, i.end)?;
        write_json(
            &asset(&s.store, &j.view.id, "source.json")?,
            &json!({"title":title,"url":url,"start":i.start,"end":i.end}),
        )
        .await?;
        // An interrupted external FFmpeg section is not byte-resumable. Discard only
        // this job's unfinished source files; completed clip.mp4 is kept above.
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("source.") && name != "source.json" {
                reject_link(&entry.path())?;
                if entry.file_type()?.is_file() {
                    std::fs::remove_file(entry.path())?;
                }
            }
        }
        j.view.stage = "Downloading selected YouTube clip".into();
        j.view.progress = 15.;
        save(s, j).await?;
        let mut args = prefix;
        args.extend(defaults());
        args.extend([
            "--newline".into(),
            "--progress".into(),
            "--progress-template".into(),
            "download:STORYFOLD_PROGRESS:%(progress._percent_str)s".into(),
            "--max-filesize".into(),
            "2G".into(),
            "--limit-rate".into(),
            "20M".into(),
            "-f".into(),
            "bv*[height<=1080]+ba/b[height<=1080]".into(),
            "--merge-output-format".into(),
            "mkv".into(),
            "--remux-video".into(),
            "mkv".into(),
            "--download-sections".into(),
            format!("*{:.3}-{:.3}", i.start, i.end).into(),
            "--force-keyframes-at-cuts".into(),
            "--output".into(),
            dir.join("source.%(ext)s").into_os_string(),
            "--".into(),
            url.into(),
        ]);
        let _ = tool(
            &bin,
            &args,
            &dir,
            cancel,
            Duration::from_secs(1800),
            Some((s, j)),
        )
        .await?;
        let source = asset(&s.store, &j.view.id, "source.mkv")?;
        if !source.is_file() {
            return Err(AppError::bad(
                "YouTube did not supply a downloadable clip. Private, protected, unavailable, or restricted videos require an authorized original file.",
            ));
        }
        j.view.stage = "Preparing H264/AAC clip for the recording bin".into();
        j.view.progress = 75.;
        save(s, j).await?;
        normalize(&source, &normalized, i.end - i.start, cancel).await?;
        title
    };
    cancelled(cancel)?;
    j.view.stage = "Adding clip to recording studio".into();
    j.view.progress = 95.;
    save(s, j).await?;
    recordings::import_take(s, &i.recording_id, &j.view.id, &normalized, &title).await?;
    // Source and clip are retained for cancellation/retry and to avoid network re-download.
    Ok(())
}
async fn probe(file: &Path, cancel: &CancellationToken) -> Result<Value> {
    let out = crate::media::run(
        Path::new("ffprobe"),
        &[
            "-v".into(),
            "error".into(),
            "-show_streams".into(),
            "-show_format".into(),
            "-of".into(),
            "json".into(),
            file.to_string_lossy().into_owned(),
        ],
        cancel,
        Duration::from_secs(30),
        None,
        true,
        None,
    )
    .await?;
    serde_json::from_slice(&out).map_err(Into::into)
}
async fn normalize(
    source: &Path,
    target: &Path,
    duration: f64,
    cancel: &CancellationToken,
) -> Result<()> {
    let info = probe(source, cancel).await?;
    let streams = info["streams"]
        .as_array()
        .ok_or_else(|| AppError::bad("YouTube clip streams could not be read."))?;
    if !streams.iter().any(|v| v["codec_type"] == "video") {
        return Err(AppError::bad("The YouTube clip has no video track."));
    }
    let audio = streams.iter().any(|v| v["codec_type"] == "audio");
    let pending = target.with_extension("pending.mp4");
    reject_link(&pending)?;
    let mut args = vec!["-i".into(), source.to_string_lossy().into_owned()];
    if !audio {
        args.extend([
            "-f".into(),
            "lavfi".into(),
            "-i".into(),
            "anullsrc=channel_layout=stereo:sample_rate=48000".into(),
        ]);
    }
    args.extend(["-map".into(),"0:v:0".into(),"-map".into(),if audio{"0:a:0"}else{"1:a:0"}.into(),"-t".into(),format!("{duration:.3}"),"-vf".into(),"scale=w='min(1920,iw)':h='min(1080,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,fps=30,setsar=1".into(),"-c:v".into(),"libx264".into(),"-preset".into(),"veryfast".into(),"-crf".into(),"20".into(),"-threads".into(),"2".into(),"-pix_fmt".into(),"yuv420p".into(),"-c:a".into(),"aac".into(),"-ar".into(),"48000".into(),"-ac".into(),"2".into(),"-movflags".into(),"+faststart".into(),"-shortest".into(),pending.to_string_lossy().into_owned()]);
    crate::media::ff(args, cancel).await?;
    validate_normalized(&pending, duration, cancel).await?;
    tokio::fs::rename(pending, target).await?;
    Ok(())
}
async fn validate_normalized(file: &Path, duration: f64, cancel: &CancellationToken) -> Result<()> {
    let value = probe(file, cancel).await?;
    let streams = value["streams"]
        .as_array()
        .ok_or_else(|| AppError::bad("Imported clip is unreadable."))?;
    let actual = value["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|n| n.is_finite())
        .ok_or_else(|| AppError::bad("Imported clip has no duration."))?;
    if (actual - duration).abs() > 0.5
        || !streams.iter().any(|s| s["codec_name"] == "h264")
        || !streams.iter().any(|s| s["codec_name"] == "aac")
    {
        return Err(AppError::bad(
            "YouTube clip timing or media format did not match the requested range. Use an authorized original file for this clip.",
        ));
    }
    Ok(())
}
fn clean_environment(cmd: &mut Command) {
    for (key, _) in std::env::vars_os() {
        let text = key.to_string_lossy().to_ascii_uppercase();
        if text.contains("TOKEN")
            || text.contains("SECRET")
            || text.contains("PASSWORD")
            || text.contains("CREDENTIAL")
            || text.ends_with("_KEY")
            || text == "PYTHONPATH"
            || text == "PYTHONHOME"
        {
            cmd.env_remove(key);
        }
    }
}
async fn capture(
    mut reader: impl tokio::io::AsyncRead + Unpin,
    limit: usize,
    strict: bool,
) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut buf = [0; 8192];
    loop {
        let n = reader.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if result.len() + n > limit {
            if strict {
                return Err(AppError::bad("YouTube importer returned too much output."));
            }
            continue;
        }
        result.extend_from_slice(&buf[..n]);
    }
    Ok(result)
}
async fn kill_tree(child: &mut tokio::process::Child) {
    if let Some(id) = child.id() {
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .args(["-TERM", "--", &format!("-{id}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
            let _ = Command::new("kill")
                .args(["-KILL", "--", &format!("-{id}")])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
        #[cfg(windows)]
        {
            let _ = Command::new("taskkill")
                .args(["/PID", &id.to_string(), "/T", "/F"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await;
        }
    }
    let _ = child.kill().await;
    let _ = child.wait().await;
}
async fn tool(
    bin: &Path,
    args: &[OsString],
    dir: &Path,
    cancel: &CancellationToken,
    timeout: Duration,
    mut job: Option<(&AppState, &mut SavedJob)>,
) -> Result<Vec<u8>> {
    let mut cmd = Command::new(bin);
    cmd.args(args)
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    clean_environment(&mut cmd);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.as_std_mut().process_group(0);
    }
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child=cmd.spawn().map_err(|_|AppError::bad("The YouTube importer could not start. Run python3 scripts/setup-youtube.py once and retry."))?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut out = tokio::spawn(capture(stdout, 16 * 1024 * 1024, true));
    let mut err = tokio::spawn(capture(stderr, 256 * 1024, false));
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let result = loop {
        tokio::select! {
            _=cancel.cancelled()=>break Err(AppError::bad("Cancelled.")),
            _=&mut deadline=>break Err(AppError::bad("YouTube import timed out. Try a shorter clip or retry later.")),
            status=child.wait()=>break status.map_err(AppError::from).and_then(|status|if status.success(){Ok(())}else{Err(AppError::bad("YouTube could not provide this clip. Update the importer, check network access, or use an authorized original file. Restricted videos and live streams cannot be imported."))}),
            _=tick.tick()=>{
                let check=async {let total=std::fs::read_dir(dir)?.try_fold(0_u64,|sum,e|{let e=e?;let meta=e.metadata()?;Ok::<_,std::io::Error>(sum+if meta.is_file(){meta.len()}else{0})})?;
                    if total>2*1024*1024*1024 {return Err(AppError::bad("YouTube clip import exceeded its 2 GiB temporary storage limit."));}
                    if let Some((s,j))=job.as_mut(){j.view.bytes_sent=total;save(s,j).await?;}Ok::<_,AppError>(())}.await;
                if let Err(error)=check{break Err(error);}
            }
        }
    };
    if result.is_err() {
        kill_tree(&mut child).await;
        out.abort();
        err.abort();
        return result.map(|_| Vec::new());
    }
    let stdout = (&mut out)
        .await
        .map_err(|_| AppError::bad("YouTube importer output was interrupted."))??;
    let _ = (&mut err).await;
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_video_url_is_canonical_and_never_a_host_or_playlist_instruction() {
        // Older FFmpeg builds default to unverified TLS. Every downloader input
        // must opt into verification even though yt-dlp's own HTTPS is verified.
        assert!(
            defaults()
                .windows(2)
                .any(|pair| pair[0].to_str() == Some("--downloader-args")
                    && pair[1].to_str() == Some("ffmpeg_i:-tls_verify 1"))
        );
        for u in [
            "https://youtu.be/BaW_jenozKc?si=tracking",
            "https://www.youtube.com/watch?v=BaW_jenozKc&list=PL123&redirect=http://127.0.0.1",
            "https://m.youtube.com/shorts/BaW_jenozKc",
            "https://youtube.com/embed/BaW_jenozKc",
        ] {
            assert_eq!(
                canonical_url(u).unwrap(),
                "https://www.youtube.com/watch?v=BaW_jenozKc"
            );
        }
        for u in [
            "file:///etc/passwd",
            "http://127.0.0.1:4311/api/session",
            "https://youtube.com.evil.test/watch?v=BaW_jenozKc",
            "https://evil@youtube.com/watch?v=BaW_jenozKc",
            "https://youtube.com/watch?v=BaW_jenozKc&v=anotherid12",
            "https://youtube.com/playlist?list=PL123",
            "https://youtube.com/watch?v=../../hello",
            "https://youtube.com:8642/watch?v=BaW_jenozKc",
        ] {
            assert!(canonical_url(u).is_err(), "accepted unsafe URL");
        }
        assert!(validate_range(0., 600.).is_ok());
        assert!(validate_range(1., 601.1).is_err());
        assert!(validate_range(f64::NAN, 5.).is_err());
        let m = json!({"id":"BaW_jenozKc","duration":10.,"title":"Example"});
        assert!(info(&m, "https://www.youtube.com/watch?v=BaW_jenozKc", 1., 3.).is_ok());
        assert!(info(&m, "https://www.youtube.com/watch?v=BaW_jenozKc", 1., 11.).is_err());
        let live = json!({"id":"BaW_jenozKc","duration":10.,"is_live":true});
        assert!(info(&live, "https://www.youtube.com/watch?v=BaW_jenozKc", 0., 2.).is_err());
    }
    #[tokio::test]
    #[ignore = "Opt-in generated FFmpeg integration, no network or physical camera"]
    async fn generated_video_and_silent_sources_normalize_to_exact_usable_clips() {
        let temp = tempfile::tempdir().unwrap();
        let cancel = CancellationToken::new();
        for audio in [false, true] {
            let source = temp.path().join(format!("source-{audio}.mkv"));
            let target = temp.path().join(format!("clip-{audio}.mp4"));
            let mut args = vec![
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "testsrc2=s=320x180:r=30".into(),
            ];
            if audio {
                args.extend([
                    "-f".into(),
                    "lavfi".into(),
                    "-i".into(),
                    "sine=frequency=440:sample_rate=48000".into(),
                ]);
            }
            args.extend([
                "-t".into(),
                "1".into(),
                "-c:v".into(),
                "libx264".into(),
                "-threads".into(),
                "1".into(),
                source.to_string_lossy().into_owned(),
            ]);
            crate::media::ff(args, &cancel).await.unwrap();
            normalize(&source, &target, 1., &cancel).await.unwrap();
            validate_normalized(&target, 1., &cancel).await.unwrap();
        }
    }
}
