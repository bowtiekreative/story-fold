use crate::{Project, Sentence, TimelineItem, Word};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Clip {
    pub sentence: Sentence,
    pub item_index: usize,
    pub dur: f64,
    pub a_start: f64,
    pub a_end: f64,
    pub l: f64,
    pub v_start: f64,
    pub v_end: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: f64,
    pub end: f64,
    pub count: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sequence {
    pub clips: Vec<Clip>,
    pub spans: Vec<Span>,
    pub total: f64,
}
impl Sequence {
    pub fn new(project: &Project) -> Self {
        sequence(project)
    }
    pub fn frame(&self, time: f64) -> (Option<&Clip>, Option<&Clip>) {
        let frame = frame_at(self, time);
        (frame.audio, frame.video)
    }
}

pub fn sequence(project: &Project) -> Sequence {
    let mut clips: Vec<Clip> = Vec::new();
    let mut total = 0.;
    for (item_index, item) in project.timeline.iter().enumerate() {
        let mut list: Vec<_> = project
            .sentences
            .iter()
            .filter(|s| {
                !s.skip
                    && match item {
                        TimelineItem::Group { topic_id } => s.topic_id == *topic_id,
                        TimelineItem::Clip { sentence_id } => s.id == *sentence_id,
                    }
            })
            .collect();
        if matches!(item, TimelineItem::Group { .. }) {
            list.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        for s in list {
            let dur = (s.out - s.r#in).max(0.3);
            clips.push(Clip {
                sentence: s.clone(),
                item_index,
                dur,
                a_start: total,
                a_end: total + dur,
                l: 0.,
                v_start: 0.,
                v_end: 0.,
            });
            total += dur;
        }
    }
    for i in 0..clips.len() {
        let c = &clips[i];
        let l = clips.get(i + 1).map_or(0., |n| {
            c.sentence.lcut.min(c.dur - 0.3).min(n.dur - 0.3).max(0.)
        });
        clips[i].l = l;
    }
    for i in 0..clips.len() {
        clips[i].v_start = clips[i].a_start - if i > 0 { clips[i - 1].l } else { 0. };
        clips[i].v_end = clips[i].a_end - clips[i].l;
    }
    let mut cursor = 0.;
    let spans = project
        .timeline
        .iter()
        .enumerate()
        .map(|(i, _)| {
            let list: Vec<_> = clips.iter().filter(|c| c.item_index == i).collect();
            let start = list.first().map_or(cursor, |c| c.a_start);
            cursor = list.last().map_or(cursor, |c| c.a_end);
            Span {
                start,
                end: cursor,
                count: list.len(),
            }
        })
        .collect();
    Sequence {
        clips,
        spans,
        total,
    }
}

#[derive(Debug)]
pub struct Frame<'a> {
    pub audio: Option<&'a Clip>,
    pub video: Option<&'a Clip>,
    pub audio_time: f64,
    pub video_time: f64,
    pub lcut: bool,
}
pub fn frame_at(sq: &Sequence, t: f64) -> Frame<'_> {
    let bounded = t.max(0.).min((sq.total - 0.00001).max(0.));
    let a = sq
        .clips
        .iter()
        .position(|c| bounded >= c.a_start && bounded < c.a_end);
    let v = sq
        .clips
        .iter()
        .position(|c| bounded >= c.v_start && bounded < c.v_end);
    let audio = a.map(|i| &sq.clips[i]);
    let video = v.map(|i| &sq.clips[i]);
    Frame {
        audio,
        video,
        audio_time: audio.map_or(0., |c| c.sentence.r#in + bounded - c.a_start),
        video_time: video.map_or(0., |c| c.sentence.r#in + bounded - c.v_start),
        lcut: a != v,
    }
}
pub fn fmt(t: f64) -> String {
    let n = (t.max(0.) * 10.).round() as u64;
    format!("{:02}:{:02}.{}", n / 600, n / 10 % 60, n % 10)
}
pub fn tc25(t: f64) -> String {
    let n = (t.max(0.) * 25.).round() as u64;
    format!(
        "{:02}:{:02}:{:02}:{:02}",
        n / 90000,
        n / 1500 % 60,
        n / 25 % 60,
        n % 25
    )
}
pub fn split_sentences(words: &[Word], duration: f64) -> Result<Vec<Sentence>, String> {
    if words.is_empty() {
        return Err("The transcript contains no timed words.".into());
    }
    if words.len() > 200000
        || words.iter().enumerate().any(|(i, w)| {
            w.text.trim().is_empty()
                || w.text.chars().count() > 200
                || !w.start.is_finite()
                || !w.end.is_finite()
                || w.start < 0.
                || w.end <= w.start
                || w.end > duration + 0.15
                || w.end - w.start > 20.
                || (i > 0 && w.start < words[i - 1].start)
        })
    {
        return Err(
            "Words must be ordered, have valid start/end timestamps, and fit inside the video."
                .into(),
        );
    }
    let mut groups: Vec<Vec<Word>> = Vec::new();
    let mut pending: Vec<Word> = Vec::new();
    for word in words {
        while !pending.is_empty() && word.end - pending[0].start > 20. {
            let mut split = 0;
            let mut gap = 0.7;
            for i in 1..pending.len() {
                let pause = pending[i].start - pending[i - 1].end;
                if pause >= gap {
                    split = i;
                    gap = pause;
                }
            }
            if split == 0 {
                split = (1..pending.len())
                    .rev()
                    .find(|&i| pending[i - 1].text.ends_with([',', ';', ':']))
                    .unwrap_or(pending.len());
            }
            let tail = pending.split_off(split);
            groups.push(pending);
            pending = tail;
        }
        pending.push(word.clone());
        if word
            .text
            .trim_end_matches(['"', '\'', ')', '”', '’'])
            .ends_with(['.', '!', '?'])
        {
            groups.push(std::mem::take(&mut pending));
        }
    }
    if !pending.is_empty() {
        groups.push(pending);
    }
    let mut i = 0;
    while i < groups.len() {
        let start = groups[i][0].start;
        let end = groups[i].last().unwrap().end;
        if end - start >= 0.8 || groups.len() == 1 {
            i += 1;
            continue;
        }
        if i + 1 < groups.len() && groups[i + 1].last().unwrap().end - start <= 20. {
            let mut next = groups.remove(i + 1);
            groups[i].append(&mut next);
        } else if i > 0 && end - groups[i - 1][0].start <= 20. {
            let mut short = groups.remove(i);
            groups[i - 1].append(&mut short);
            i -= 1;
        } else {
            i += 1;
        }
    }
    Ok(groups
        .iter()
        .enumerate()
        .map(|(i, g)| {
            let start = g[0].start;
            let end = g.last().unwrap().end;
            Sentence {
                id: format!("s{}", i + 1),
                n: (i + 1) as u32,
                text: g
                    .iter()
                    .map(|w| w.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" "),
                words: g.clone(),
                start,
                end,
                r#in: (start - 0.15).max(0.),
                out: duration
                    .min(end + 0.1)
                    .min(groups.get(i + 1).map_or(f64::INFINITY, |g| g[0].start)),
                topic_id: "interview".into(),
                cam: "A".into(),
                ..Default::default()
            }
        })
        .collect())
}
