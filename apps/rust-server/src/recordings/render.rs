use super::*;
use std::path::Path;

fn clean(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, '{' | '}' | '\\' | '\r'))
        .collect::<String>()
        .lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\\N")
}
fn wrap(text: &str, columns: usize) -> String {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            if !line.is_empty() && line.chars().count() + word.chars().count() + 1 > columns {
                lines.push(line);
                line = String::new();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    clean(&lines.join("\n"))
}
fn overlays(session: &RecordingSession, clip: &RecordingClip, w: u32, h: u32) -> String {
    let mut ass = format!(
        "[Script Info]\nScriptType: v4.00+\nPlayResX: {w}\nPlayResY: {h}\nWrapStyle: 2\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Card,Arial,48,&H003B2632,&H003B2632,&H00FFFFFF,&H28FFFFFF,0,0,0,0,100,100,0,0,3,27,0,7,72,72,60,1\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"
    );
    let Some(take) = session.takes.iter().find(|t| t.id == clip.take_id) else {
        return ass;
    };
    for (i, mark) in take.marks.iter().enumerate() {
        let start = mark.at.max(clip.r#in);
        let end = take
            .marks
            .get(i + 1)
            .map_or(take.duration, |m| m.at)
            .min(clip.out);
        if end <= start {
            continue;
        }
        let Some(card) = session
            .overlays
            .iter()
            .find(|o| Some(&o.id) == mark.overlay_id.as_ref())
        else {
            continue;
        };
        let (alignment, x, y) = match card.kind.as_str() {
            "quote" => (2, w / 2, h * 85 / 100),
            "list" => (3, w.saturating_sub(72), h.saturating_sub(72)),
            _ => (1, 72, h.saturating_sub(72)),
        };
        let columns = if card.kind == "list" {
            if w > h { 30 } else { 20 }
        } else if w > h {
            54
        } else {
            28
        };
        let size = if card.kind == "stat" { 81 } else { 48 };
        let text = wrap(&card.text, columns);
        let secondary = wrap(&card.secondary, columns);
        let text = if secondary.is_empty() {
            text
        } else {
            format!("{text}\\N{{\\fs36}}{secondary}")
        };
        ass.push_str(&format!(
            "Dialogue: 0,{},{},Card,,0,0,0,,{{\\an{alignment}\\pos({x},{y})\\fs{size}}}{text}\n",
            media::ass_time(start - clip.r#in),
            media::ass_time(end - clip.r#in)
        ));
    }
    ass
}
async fn ff(args: Vec<String>, cwd: &Path) -> Result<()> {
    let mut all = vec![
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-y".into(),
    ];
    all.extend(args);
    media::run(
        Path::new("ffmpeg"),
        &all,
        &CancellationToken::new(),
        Duration::from_secs(7200),
        Some(cwd),
        false,
        None,
    )
    .await?;
    Ok(())
}
pub(super) async fn render(store: &Store, session: &RecordingSession, job: &str) -> Result<()> {
    let base = directory(store, &session.id)?;
    let temp = tempfile::tempdir_in(&base)?;
    let (width, height) = if session.orientation == "16:9" {
        (1920, 1080)
    } else {
        (1080, 1920)
    };
    let mut concat = String::new();
    let mut expected = 0.;
    for (i, clip) in session.clips.iter().enumerate() {
        identifier(&clip.take_id)?;
        let frames = ((clip.out - clip.r#in) * 30.).round().max(1.) as u64;
        let duration = frames as f64 / 30.;
        expected += duration;
        let take = asset(store, &session.id, &format!("take-{}.mp4", clip.take_id))?;
        tokio::fs::write(
            temp.path().join("overlay.ass"),
            overlays(session, clip, width, height),
        )
        .await?;
        let output = format!("segment-{i}.mkv");
        let filter = format!(
            "scale={width}:{height}:force_original_aspect_ratio=increase,crop={width}:{height},setsar=1,fps=30,tpad=stop_mode=clone:stop_duration={duration},trim=end_frame={frames},ass=filename=overlay.ass"
        );
        ff(vec!["-filter_threads".into(),"1".into(),"-ss".into(),clip.r#in.to_string(),"-i".into(),take.to_string_lossy().into(),"-t".into(),duration.to_string(),"-map".into(),"0:v:0".into(),"-map".into(),"0:a:0".into(),"-vf".into(),filter,"-af".into(),format!("aresample=48000,aformat=channel_layouts=stereo,apad,atrim=duration={duration},afade=t=in:d=0.005,afade=t=out:st={}:d=0.005",(duration-0.005).max(0.)),"-c:v".into(),"libx264".into(),"-threads".into(),"2".into(),"-preset".into(),"veryfast".into(),"-crf".into(),"20".into(),"-pix_fmt".into(),"yuv420p".into(),"-c:a".into(),"pcm_s16le".into(),output.clone()],temp.path()).await?;
        concat.push_str(&format!("file '{output}'\n"));
    }
    tokio::fs::write(temp.path().join("sequence.txt"), concat).await?;
    ff(
        vec![
            "-f".into(),
            "concat".into(),
            "-safe".into(),
            "0".into(),
            "-i".into(),
            "sequence.txt".into(),
            "-c:v".into(),
            "copy".into(),
            "-c:a".into(),
            "aac".into(),
            "-b:a".into(),
            "160k".into(),
            "-movflags".into(),
            "+faststart".into(),
            "sequence.mp4".into(),
        ],
        temp.path(),
    )
    .await?;
    let result = temp.path().join("sequence.mp4");
    let info = media::probe(&result).await?;
    if (info.duration - expected).abs() > 0.15 || info.width != width || info.height != height {
        return Err(AppError::bad(
            "The recording export did not match its sequence. Original takes are unchanged; try exporting again.",
        ));
    }
    tokio::fs::rename(
        result,
        asset(store, &session.id, &format!("export-{job}.mp4"))?,
    )
    .await?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overlay_intersections_follow_trims_and_strip_ass_commands() {
        let session = RecordingSession {
            takes: vec![RecordedTake {
                id: "take".into(),
                duration: 10.,
                marks: vec![
                    OverlayMark {
                        at: 1.,
                        overlay_id: Some("card".into()),
                    },
                    OverlayMark {
                        at: 6.,
                        overlay_id: None,
                    },
                ],
                ..Default::default()
            }],
            overlays: vec![RecordingOverlay {
                id: "card".into(),
                kind: "lowerThird".into(),
                text: "Hello {\\p1} world\nNext line".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let clip = RecordingClip {
            take_id: "take".into(),
            r#in: 3.,
            out: 8.,
            ..Default::default()
        };
        let ass = overlays(&session, &clip, 1280, 720);
        assert!(ass.contains("0:00:00.00,0:00:03.00"));
        assert!(!ass.contains("{\\p1}"));
        assert!(ass.contains("world\\NNext"));
    }
}
