use crate::{Project, sequence, tc25};
use std::fmt::Write;

fn srt_time(t: f64) -> String {
    let n = (t.max(0.) * 1000.).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        n / 3600000,
        n / 60000 % 60,
        n / 1000 % 60,
        n % 1000
    )
}
pub fn build_srt(p: &Project) -> String {
    sequence(p)
        .clips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                i + 1,
                srt_time(c.a_start),
                srt_time(c.a_end),
                c.sentence.text.replace(['\r', '\n'], " ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn build_edl(p: &Project) -> String {
    let mut out = format!(
        "TITLE: {}\nFCM: NON-DROP FRAME\n\n",
        p.name.replace(['\r', '\n'], " ")
    );
    let mut n = 1;
    for c in sequence(p).clips {
        for track in ["V", "A"] {
            let id = if track == "V" { &c.sentence.cam } else { "A" };
            let Some(cam) = p.cameras.iter().find(|cam| cam.id == id) else {
                continue;
            };
            let (start, end) = if track == "V" {
                (c.v_start, c.v_end)
            } else {
                (c.a_start, c.a_end)
            };
            let source = c.sentence.r#in + cam.offset;
            let topic = p
                .topics
                .iter()
                .find(|t| t.id == c.sentence.topic_id)
                .map_or("", |t| t.name.as_str());
            let _ = writeln!(
                out,
                "{n:03}  CAM{}     {track}     C        {} {} {} {}\n* FROM CLIP NAME: {}\n* {}: {}",
                if cam.virtual_crop.is_some() {
                    "A"
                } else {
                    &cam.id
                },
                tc25(source),
                tc25(source + end - start),
                tc25(start),
                tc25(end),
                cam.source_path.rsplit('/').next().unwrap_or(""),
                topic,
                c.sentence.text.replace(['\r', '\n'], " ")
            );
            if let Some(crop) = &cam.virtual_crop
                && track == "V"
            {
                let _ = writeln!(
                    out,
                    "* VIRTUAL CROP {}: scale {}; reapply crop in your NLE",
                    cam.id, crop.scale
                );
            }
            out.push('\n');
            n += 1;
        }
    }
    // Keep the original EDL's final newline convention.
    out.pop();
    out
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn rational(n: f64) -> String {
    format!("{}/25000s", (n * 25000.).round() as i64)
}
fn uri_component(s: &str) -> String {
    s.bytes()
        .map(|c| {
            if c.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&c) {
                (c as char).to_string()
            } else {
                format!("%{c:02X}")
            }
        })
        .collect()
}
pub fn build_fcpxml(p: &Project) -> String {
    let sq = sequence(p);
    let mut resources = String::new();
    let mut spine = String::new();
    for (i, c) in p.cameras.iter().enumerate() {
        let _ = write!(
            resources,
            "<asset id=\"r{}\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"1\" hasAudio=\"1\" audioSources=\"1\" audioChannels=\"2\" format=\"r1\"><media-rep kind=\"original-media\" src=\"file:///Storyfold/{}\"/></asset>",
            i + 2,
            xml(&c.name),
            rational(c.duration),
            uri_component(c.source_path.rsplit('/').next().unwrap_or(""))
        );
    }
    for c in &sq.clips {
        let Some((i, cam)) = p
            .cameras
            .iter()
            .enumerate()
            .find(|(_, cam)| cam.id == c.sentence.cam)
        else {
            continue;
        };
        let scale = cam.virtual_crop.as_ref().map_or(1., |c| c.scale);
        let _ = write!(
            spine,
            "<asset-clip ref=\"r{}\" name=\"{}\" offset=\"{}\" start=\"{}\" duration=\"{}\" audioRole=\"dialogue\" srcEnable=\"video\"><adjust-transform scale=\"{scale} {scale}\"/>",
            i + 2,
            xml(&c.sentence.text),
            rational(c.v_start),
            rational(c.sentence.r#in + cam.offset),
            rational(c.v_end - c.v_start)
        );
        for a in sq
            .clips
            .iter()
            .filter(|a| a.a_start >= c.v_start && a.a_start < c.v_end)
        {
            let _ = write!(
                spine,
                "<asset-clip ref=\"r2\" lane=\"-1\" offset=\"{}\" start=\"{}\" duration=\"{}\" srcEnable=\"audio\" audioRole=\"dialogue\"/>",
                rational(c.sentence.r#in + cam.offset + a.a_start - c.v_start),
                rational(a.sentence.r#in),
                rational(a.dur)
            );
        }
        spine.push_str("</asset-clip>");
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><!DOCTYPE fcpxml><fcpxml version=\"1.10\"><resources><format id=\"r1\" name=\"FFVideoFormat720p25\" frameDuration=\"1/25s\" width=\"1280\" height=\"720\"/>{resources}</resources><library><event name=\"Storyfold\"><project name=\"{}\"><sequence format=\"r1\" duration=\"{}\" tcStart=\"0s\" tcFormat=\"NDF\" audioLayout=\"stereo\" audioRate=\"48k\"><spine>{spine}</spine></sequence></project></event></library></fcpxml>",
        xml(&p.name),
        rational(sq.total)
    )
}
