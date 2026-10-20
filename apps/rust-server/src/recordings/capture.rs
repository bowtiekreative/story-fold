use super::*;
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Command,
};

fn strings(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}
#[derive(Clone)]
pub(super) struct Input {
    pub args: Vec<String>,
    pub audio: usize,
}
pub(super) fn test_source() -> bool {
    cfg!(debug_assertions) && std::env::var("STORYFOLD_CAPTURE_TEST").is_ok_and(|s| s == "1")
}
#[cfg(target_os = "linux")]
mod linux {
    use std::{
        fs::OpenOptions,
        io,
        os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
        path::Path,
    };

    #[repr(C)]
    #[derive(Default)]
    struct Capability {
        driver: [u8; 16],
        card: [u8; 32],
        bus_info: [u8; 32],
        version: u32,
        capabilities: u32,
        device_caps: u32,
        reserved: [u32; 3],
    }
    fn captures_video(capabilities: u32, device_caps: u32) -> bool {
        let flags = if capabilities & 0x8000_0000 != 0 {
            device_caps
        } else {
            capabilities
        };
        flags & (0x0000_0001 | 0x0000_1000) != 0
    }
    pub(super) fn camera(path: &Path) -> io::Result<Option<String>> {
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        let mut capability = Capability::default();
        // QUERYCAP reads the node's capabilities; it never queues buffers or starts a stream.
        let result = unsafe {
            libc::ioctl(
                file.as_raw_fd(),
                libc::_IOR::<Capability>(b'V' as u32, 0),
                &mut capability,
            )
        };
        if result < 0 {
            return Err(io::Error::last_os_error());
        }
        if !captures_video(capability.capabilities, capability.device_caps) {
            return Ok(None);
        }
        let length = capability
            .card
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(capability.card.len());
        Ok(Some(
            String::from_utf8_lossy(&capability.card[..length])
                .chars()
                .filter(|c| !c.is_control())
                .collect(),
        ))
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn per_node_capabilities_exclude_uvc_metadata_and_output_nodes() {
            assert_eq!(std::mem::size_of::<Capability>(), 104);
            assert_eq!(std::mem::offset_of!(Capability, device_caps), 88);
            assert!(captures_video(0x84a0_0001, 0x0420_0001));
            assert!(!captures_video(0x84a0_0001, 0x04a0_0000));
            assert!(!captures_video(0x8000_0002, 2));
            assert!(captures_video(1, 0));
            assert!(captures_video(0x8000_1000, 0x1000));
        }
    }
}
async fn inventory() -> Result<(
    RecordingDevices,
    HashMap<String, String>,
    HashMap<String, String>,
)> {
    let mut result = RecordingDevices::default();
    let mut video = HashMap::new();
    let mut audio = HashMap::new();
    if test_source() {
        result.cameras.push(RecordingDevice {
            id: "synthetic".into(),
            name: "Synthetic test pattern (automation only)".into(),
        });
        video.insert("synthetic".into(), "synthetic".into());
        result.microphones.push(RecordingDevice {
            id: "synthetic-audio".into(),
            name: "Generated test tone (automation only)".into(),
        });
        audio.insert("synthetic-audio".into(), "synthetic".into());
        result.message =
            "Generated test source; physical devices are excluded in this debug mode.".into();
        return Ok((result, video, audio));
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::FileTypeExt;
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let id = entry.file_name().to_string_lossy().into_owned();
                // Enumerate accessible nodes directly; sysfs may be unavailable in an app sandbox.
                if id.starts_with("video")
                    && id.len() > 5
                    && id[5..].bytes().all(|b| b.is_ascii_digit())
                    && entry.file_type().is_ok_and(|kind| kind.is_char_device())
                {
                    match linux::camera(&entry.path()) {
                        Ok(Some(name)) => {
                            result.cameras.push(RecordingDevice {
                                id: id.clone(),
                                name: if name.is_empty() {
                                    id.clone()
                                } else {
                                    format!("{name} ({id})")
                                },
                            });
                            video.insert(id.clone(), format!("/dev/{id}"));
                        }
                        Ok(None) => {}
                        Err(error) => {
                            result.message = if error.kind() == std::io::ErrorKind::PermissionDenied {
                                "A camera was found, but Linux denied access. Check camera permissions for your account, then refresh devices."
                            } else {
                                "A camera node could not be queried. Close other camera apps and refresh devices."
                            }.into();
                        }
                    }
                }
            }
        }
        result.microphones.push(RecordingDevice {
            id: "default".into(),
            name: "Default system microphone".into(),
        });
        audio.insert("default".into(), "default".into());
    }
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    {
        let format = if cfg!(windows) {
            "dshow"
        } else {
            "avfoundation"
        };
        let mut cmd = Command::new("ffmpeg");
        cmd.args([
            "-hide_banner",
            "-f",
            format,
            "-list_devices",
            "true",
            "-i",
            if cfg!(windows) { "dummy" } else { "" },
        ]);
        #[cfg(windows)]
        cmd.creation_flags(0x08000000);
        let out = tokio::time::timeout(Duration::from_secs(8), cmd.kill_on_drop(true).output())
            .await
            .map_err(|_| {
                AppError::bad("Camera discovery took too long. Check the device connection.")
            })??;
        let listing = String::from_utf8_lossy(&out.stderr);
        let mut section = "";
        for line in listing.lines().take(1000) {
            if line.contains("AVFoundation video devices") {
                section = "video";
                continue;
            }
            if line.contains("AVFoundation audio devices") {
                section = "audio";
                continue;
            }
            let parsed = if cfg!(windows) {
                let kind = if line.contains("(video)") {
                    "video"
                } else if line.contains("(audio)") {
                    "audio"
                } else {
                    continue;
                };
                line.split('"')
                    .nth(1)
                    .map(|name| (kind.to_string(), name.to_string(), name.to_string()))
            } else {
                line.rsplit_once('[')
                    .and_then(|(_, tail)| tail.split_once("] "))
                    .filter(|(index, _)| index.bytes().all(|b| b.is_ascii_digit()))
                    .map(|(index, name)| (section.to_string(), index.into(), name.trim().into()))
            };
            if let Some((kind, value, name)) = parsed {
                if kind == "video" && !name.to_lowercase().contains("capture screen") {
                    let id = format!("camera-{}", video.len());
                    video.insert(id.clone(), value);
                    result.cameras.push(RecordingDevice { id, name });
                } else if kind == "audio" {
                    let id = format!("mic-{}", audio.len());
                    audio.insert(id.clone(), value);
                    result.microphones.push(RecordingDevice { id, name });
                }
            }
        }
    }
    result.cameras.sort_by(|a, b| a.id.cmp(&b.id));
    if result.message.is_empty() {
        result.message = if result.cameras.is_empty() {
            "No webcam detected. Connect a camera, close other camera apps, then refresh devices. Recording starts only when you press Record.".into()
        } else {
            "Camera and microphone stay off until you start a take. Recordings stay on this computer."
            .into()
        };
    }
    Ok((result, video, audio))
}
pub(super) async fn devices() -> Result<RecordingDevices> {
    Ok(inventory().await?.0)
}
pub(super) async fn input(camera: &str, microphone: Option<&str>) -> Result<Input> {
    let (_, videos, audios) = inventory().await?;
    let video = videos.get(camera).ok_or_else(|| {
        AppError::bad("That camera is unavailable. Connect it and refresh the device list.")
    })?;
    let audio = microphone
        .map(|id| {
            audios.get(id).ok_or_else(|| {
                AppError::bad(
                    "That microphone is unavailable. Choose a detected microphone or turn it off.",
                )
            })
        })
        .transpose()?;
    let mut args = Vec::new();
    let mut audio_index = 0;
    if camera == "synthetic" && test_source() {
        args = strings(&["-re", "-f", "lavfi", "-i", "testsrc2=size=640x360:rate=25"]);
        if audio.is_some() {
            args.extend(strings(&[
                "-re",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:sample_rate=48000",
            ]));
            audio_index = 1;
        }
    } else {
        #[cfg(target_os = "linux")]
        {
            args.extend(strings(&[
                "-thread_queue_size",
                "512",
                "-f",
                "v4l2",
                "-i",
                video,
            ]));
            if let Some(audio) = audio {
                args.extend(strings(&[
                    "-thread_queue_size",
                    "512",
                    "-f",
                    "pulse",
                    "-i",
                    audio,
                ]));
                audio_index = 1;
            }
        }
        #[cfg(target_os = "windows")]
        {
            let source = if let Some(audio) = audio {
                format!("video={video}:audio={audio}")
            } else {
                format!("video={video}")
            };
            args.extend(strings(&[
                "-thread_queue_size",
                "512",
                "-f",
                "dshow",
                "-i",
                &source,
            ]));
        }
        #[cfg(target_os = "macos")]
        {
            let source = format!("{}:{}", video, audio.map(String::as_str).unwrap_or("none"));
            args.extend(strings(&[
                "-thread_queue_size",
                "512",
                "-f",
                "avfoundation",
                "-i",
                &source,
            ]));
        }
    }
    if audio.is_none() {
        args.extend(strings(&[
            "-re",
            "-f",
            "lavfi",
            "-i",
            "anullsrc=channel_layout=stereo:sample_rate=48000",
        ]));
        audio_index = 1;
    }
    Ok(Input {
        args,
        audio: audio_index,
    })
}

pub(super) async fn run(
    store: &Store,
    id: &str,
    take: &str,
    input: Input,
    live: &Live,
    frames: watch::Sender<Vec<u8>>,
) -> Result<(f64, bool)> {
    let raw = asset(store, id, &format!("take-{take}.mkv"))?;
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-hide_banner","-loglevel","error","-y","-filter_complex_threads","1","-stats_period","0.1","-progress","pipe:2"])
        .args(input.args)
        .args(["-filter_complex","[0:v]scale=1280:720:force_original_aspect_ratio=decrease,setsar=1,split=2[recorded][p];[p]fps=6,scale=640:-2[preview]","-map","[recorded]","-map",&format!("{}:a:0",input.audio),"-c:v","libx264","-preset","veryfast","-crf","20","-pix_fmt","yuv420p","-r","25","-c:a","aac","-b:a","128k","-f","matroska"])
        .arg(&raw)
        .args(["-map","[preview]","-an","-c:v","mjpeg","-threads","1","-q:v","6","-f","image2pipe","pipe:1"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);
    let mut child = cmd.spawn().map_err(|_| {
        AppError::bad("FFmpeg could not start. Check that the media tools are installed.")
    })?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| AppError::bad("Camera preview is unavailable."))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| AppError::bad("Capture progress is unavailable."))?;
    let frame_task = tokio::spawn(jpeg_frames(stdout, frames));
    let clock = live.micros.clone();
    let progress_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(value) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<u64>().ok())
            {
                clock.fetch_max(value, Ordering::Relaxed);
            }
        }
    });
    let limit = tokio::time::sleep(Duration::from_secs(7200));
    tokio::pin!(limit);
    let result = tokio::select! {
        status=child.wait()=>status.map_err(AppError::from),
        _=live.cancel.cancelled()=>stop_child(&mut child).await,
        _=&mut limit=>stop_child(&mut child).await,
        _=async {loop {tokio::time::sleep(Duration::from_secs(1)).await;if std::fs::metadata(&raw).is_ok_and(|m|m.len()>4*1024*1024*1024-2*1024*1024){break;}}}=>stop_child(&mut child).await,
    };
    let _ = frame_task.await;
    let _ = progress_task.await;
    let interrupted = !result?.success();
    let duration=finalize(store,id,take).await.map_err(|_|AppError::bad("Camera or microphone capture failed. Check permissions and close other camera apps. Any captured source remains in this recording folder."))?;
    Ok((duration, interrupted))
}
async fn stop_child(child: &mut tokio::process::Child) -> Result<std::process::ExitStatus> {
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"q\n").await;
        let _ = stdin.shutdown().await;
    }
    match tokio::time::timeout(Duration::from_secs(8), child.wait()).await {
        Ok(result) => Ok(result?),
        Err(_) => {
            child.kill().await?;
            Ok(child.wait().await?)
        }
    }
}
async fn jpeg_frames(
    mut stream: impl tokio::io::AsyncRead + Unpin,
    frames: watch::Sender<Vec<u8>>,
) {
    let mut buffer = Vec::new();
    let mut bytes = [0_u8; 8192];
    while let Ok(count) = stream.read(&mut bytes).await {
        if count == 0 {
            break;
        }
        buffer.extend_from_slice(&bytes[..count]);
        while let Some(start) = buffer.windows(2).position(|w| w == [0xff, 0xd8]) {
            if start > 0 {
                buffer.drain(..start);
            }
            let Some(end) = buffer.windows(2).position(|w| w == [0xff, 0xd9]) else {
                break;
            };
            frames.send_replace(buffer.drain(..end + 2).collect());
        }
        if buffer.len() > 4 * 1024 * 1024 {
            buffer.clear();
        }
    }
}
pub(super) async fn finalize(store: &Store, id: &str, take: &str) -> Result<f64> {
    identifier(take)?;
    let raw = asset(store, id, &format!("take-{take}.mkv"))?;
    let output = asset(store, id, &format!("take-{take}.mp4"))?;
    if output.is_file() {
        return Ok(media::probe(&output).await?.duration);
    }
    reject_link(&raw)?;
    if !raw.is_file() {
        return Err(AppError::bad(
            "No camera frames were recorded. Check the selected device.",
        ));
    }
    let staging = asset(store, id, &format!("finalize-{take}.mp4"))?;
    media::ff(
        vec![
            "-i".into(),
            raw.to_string_lossy().into(),
            "-map".into(),
            "0".into(),
            "-c".into(),
            "copy".into(),
            "-movflags".into(),
            "+faststart".into(),
            staging.to_string_lossy().into(),
        ],
        &CancellationToken::new(),
    )
    .await?;
    let info = media::probe(&staging).await?;
    if info.duration < 0.1 {
        return Err(AppError::bad(
            "The take was too short. Record at least a moment before stopping.",
        ));
    }
    tokio::fs::rename(&staging, &output).await?;
    let _ = tokio::fs::remove_file(raw).await;
    Ok(info.duration)
}
