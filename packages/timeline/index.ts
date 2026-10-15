export type CamId = "A" | "B" | "C";
export type Accent = "violet" | "coral" | "teal" | "amber" | "sky" | "lime";
export interface Word {
  text: string;
  start: number;
  end: number;
}
export interface Camera {
  id: CamId;
  name: string;
  sourcePath: string;
  proxyPath: string;
  offset: number;
  duration: number;
  virtual?: { scale: number; x: number; y: number };
}
export interface Sentence {
  id: string;
  n: number;
  text: string;
  words: Word[];
  start: number;
  end: number;
  in: number;
  out: number;
  topicId: string;
  cam: CamId;
  lcut: number;
  skip: boolean;
}
export interface Topic {
  id: string;
  name: string;
  accent: Accent;
  summary?: string;
}
export type TimelineItem =
  { kind: "group"; topicId: string } | { kind: "clip"; sentenceId: string };
export interface ExportSettings {
  ratio: "16:9" | "9:16" | "1:1";
  captions: boolean;
  chapters: boolean;
}
export interface Progress {
  step: string;
  detail: string;
  pct: number;
  at: string;
}
export interface Project {
  id: string;
  name: string;
  revision: number;
  createdAt: string;
  status:
    | "uploading"
    | "transcribing"
    | "segmenting"
    | "syncing"
    | "grouping"
    | "ready"
    | "error";
  cameras: Camera[];
  sentences: Sentence[];
  topics: Topic[];
  timeline: TimelineItem[];
  exportSettings: ExportSettings;
  progress: Progress[];
  error?: string;
  warnings: string[];
  sample?: boolean;
  grouping?: string;
}
export interface Clip {
  sentence: Sentence;
  itemIndex: number;
  dur: number;
  aStart: number;
  aEnd: number;
  l: number;
  vStart: number;
  vEnd: number;
}
export function sequence(project: Pick<Project, "sentences" | "timeline">) {
  const clips: Clip[] = [];
  let time = 0;
  project.timeline.forEach((item, itemIndex) => {
    const list = project.sentences.filter(
      (s) =>
        !s.skip &&
        (item.kind === "group"
          ? s.topicId === item.topicId
          : s.id === item.sentenceId),
    );
    if (item.kind === "group") list.sort((a, b) => a.start - b.start);
    for (const sentence of list) {
      const dur = Math.max(0.3, sentence.out - sentence.in);
      clips.push({
        sentence,
        itemIndex,
        dur,
        aStart: time,
        aEnd: time + dur,
        l: 0,
        vStart: 0,
        vEnd: 0,
      });
      time += dur;
    }
  });
  clips.forEach((c, i) => {
    const next = clips[i + 1];
    c.l = next
      ? Math.max(0, Math.min(c.sentence.lcut, c.dur - 0.3, next.dur - 0.3))
      : 0;
  });
  clips.forEach((c, i) => {
    c.vStart = c.aStart - (clips[i - 1]?.l ?? 0);
    c.vEnd = c.aEnd - c.l;
  });
  let cursor = 0;
  const spans = project.timeline.map((_, i) => {
    const list = clips.filter((c) => c.itemIndex === i);
    const start = list[0]?.aStart ?? cursor;
    cursor = list.at(-1)?.aEnd ?? cursor;
    return { start, end: cursor, count: list.length };
  });
  return { clips, spans, total: time };
}
export function frameAt(sq: ReturnType<typeof sequence>, t: number) {
  const bounded = Math.min(Math.max(0, t), Math.max(0, sq.total - 0.00001));
  const audio = sq.clips.find((c) => bounded >= c.aStart && bounded < c.aEnd);
  const video = sq.clips.find((c) => bounded >= c.vStart && bounded < c.vEnd);
  return {
    audio,
    video,
    audioTime: audio ? audio.sentence.in + bounded - audio.aStart : 0,
    videoTime: video ? video.sentence.in + bounded - video.vStart : 0,
    lcut: audio !== video,
  };
}
export function fmt(t: number) {
  const tenths = Math.round(Math.max(0, t) * 10);
  return `${String(Math.floor(tenths / 600)).padStart(2, "0")}:${String(Math.floor(tenths / 10) % 60).padStart(2, "0")}.${tenths % 10}`;
}
export function tc25(t: number) {
  const f = Math.round(Math.max(0, t) * 25);
  return [
    Math.floor(f / 90000),
    Math.floor(f / 1500) % 60,
    Math.floor(f / 25) % 60,
    f % 25,
  ]
    .map((x) => String(x).padStart(2, "0"))
    .join(":");
}
export function splitSentences(words: Word[], duration = Infinity): Sentence[] {
  if (!words.length) throw new Error("The transcript contains no timed words.");
  if (
    words.some(
      (w, i) =>
        !w.text.trim() ||
        !Number.isFinite(w.start) ||
        !Number.isFinite(w.end) ||
        w.start < 0 ||
        w.end <= w.start ||
        w.end > duration + 0.15 ||
        w.end - w.start > 20 ||
        (i > 0 && w.start < words[i - 1].start),
    )
  )
    throw new Error(
      "Words must be ordered, have valid start/end timestamps, and fit inside the video.",
    );
  const groups: Word[][] = [];
  let pending: Word[] = [];
  for (const word of words) {
    while (pending.length && word.end - pending[0].start > 20) {
      let split = -1,
        gap = 0.7;
      for (let i = 1; i < pending.length; i++) {
        const pause = pending[i].start - pending[i - 1].end;
        if (pause >= gap) {
          split = i;
          gap = pause;
        }
      }
      if (split < 1) {
        for (let i = pending.length - 1; i > 0; i--) {
          if (/[,;:]$/.test(pending[i - 1].text)) {
            split = i;
            break;
          }
        }
      }
      if (split < 1) split = pending.length;
      groups.push(pending.slice(0, split));
      pending = pending.slice(split);
    }
    pending.push(word);
    if (/[.!?]["')”’]*$/.test(word.text)) {
      groups.push(pending);
      pending = [];
    }
  }
  if (pending.length) groups.push(pending);
  for (let i = 0; i < groups.length; i++) {
    const g = groups[i];
    if (g.at(-1)!.end - g[0].start >= 0.8 || groups.length === 1) continue;
    const next = groups[i + 1];
    const previous = groups[i - 1];
    if (next && next.at(-1)!.end - g[0].start <= 20) {
      groups.splice(i, 2, [...g, ...next]);
      i--;
    } else if (previous && g.at(-1)!.end - previous[0].start <= 20) {
      groups.splice(i - 1, 2, [...previous, ...g]);
      i--;
    }
  }
  return groups.map((g, i) => {
    const start = g[0].start,
      end = g.at(-1)!.end;
    return {
      id: `s${i + 1}`,
      n: i + 1,
      text: g.map((w) => w.text).join(" "),
      words: g,
      start,
      end,
      in: Math.max(0, start - 0.15),
      out: Math.min(duration, end + 0.1, groups[i + 1]?.[0].start ?? Infinity),
      topicId: "interview",
      cam: "A",
      lcut: 0,
      skip: false,
    };
  });
}
const srtTime = (t: number) => {
  const ms = Math.round(Math.max(0, t) * 1000);
  return `${[Math.floor(ms / 3600000), Math.floor(ms / 60000) % 60, Math.floor(ms / 1000) % 60].map((x) => String(x).padStart(2, "0")).join(":")},${String(ms % 1000).padStart(3, "0")}`;
};
export function buildSrt(project: Project) {
  return sequence(project)
    .clips.map(
      (c, i) =>
        `${i + 1}\n${srtTime(c.aStart)} --> ${srtTime(c.aEnd)}\n${c.sentence.text.replace(/\r?\n/g, " ")}\n`,
    )
    .join("\n");
}
export function buildEdl(project: Project) {
  const lines = [
    `TITLE: ${project.name.replace(/[\r\n]/g, " ")}`,
    "FCM: NON-DROP FRAME",
    "",
  ];
  let n = 1;
  for (const c of sequence(project).clips) {
    for (const track of ["V", "A"] as const) {
      const cam = project.cameras.find(
        (x) => x.id === (track === "V" ? c.sentence.cam : "A"),
      )!;
      const start = track === "V" ? c.vStart : c.aStart,
        end = track === "V" ? c.vEnd : c.aEnd;
      const source = c.sentence.in + cam.offset;
      lines.push(
        `${String(n++).padStart(3, "0")}  CAM${cam.virtual ? "A" : cam.id}     ${track}     C        ${tc25(source)} ${tc25(source + end - start)} ${tc25(start)} ${tc25(end)}`,
        `* FROM CLIP NAME: ${cam.sourcePath.split("/").at(-1)}`,
        `* ${project.topics.find((t) => t.id === c.sentence.topicId)?.name}: ${c.sentence.text.replace(/[\r\n]/g, " ")}`,
        ...(cam.virtual && track === "V"
          ? [
              `* VIRTUAL CROP ${cam.id}: scale ${cam.virtual.scale}; reapply crop in your NLE`,
            ]
          : []),
        "",
      );
    }
  }
  return lines.join("\n");
}
const xml = (s: string) =>
  s.replace(
    /[<>&"']/g,
    (c) =>
      ({
        "<": "&lt;",
        ">": "&gt;",
        "&": "&amp;",
        '"': "&quot;",
        "'": "&apos;",
      })[c]!,
  );
const rational = (n: number) => `${Math.round(n * 25000)}/25000s`;
export function buildFcpxml(project: Project) {
  const sq = sequence(project);
  const resources = project.cameras
    .map(
      (c, i) =>
        `<asset id="r${i + 2}" name="${xml(c.name)}" start="0s" duration="${rational(c.duration)}" hasVideo="1" hasAudio="1" audioSources="1" audioChannels="2" format="r1"><media-rep kind="original-media" src="file:///Storyfold/${encodeURIComponent(c.sourcePath.split("/").at(-1)!)}"/></asset>`,
    )
    .join("");
  const spine = sq.clips
    .map((c) => {
      const cameraIndex = project.cameras.findIndex(
        (x) => x.id === c.sentence.cam,
      );
      const cam = project.cameras[cameraIndex];
      const scale = cam.virtual?.scale ?? 1;
      return `<asset-clip ref="r${cameraIndex + 2}" name="${xml(c.sentence.text)}" offset="${rational(c.vStart)}" start="${rational(c.sentence.in + cam.offset)}" duration="${rational(c.vEnd - c.vStart)}" audioRole="dialogue" srcEnable="video"><adjust-transform scale="${scale} ${scale}"/>${sq.clips
        .filter((a) => a.aStart >= c.vStart && a.aStart < c.vEnd)
        .map(
          (a) =>
            `<asset-clip ref="r2" lane="-1" offset="${rational(c.sentence.in + cam.offset + a.aStart - c.vStart)}" start="${rational(a.sentence.in)}" duration="${rational(a.dur)}" srcEnable="audio" audioRole="dialogue"/>`,
        )
        .join("")}</asset-clip>`;
    })
    .join("");
  return `<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE fcpxml><fcpxml version="1.10"><resources><format id="r1" name="FFVideoFormat720p25" frameDuration="1/25s" width="1280" height="720"/>${resources}</resources><library><event name="Storyfold"><project name="${xml(project.name)}"><sequence format="r1" duration="${rational(sq.total)}" tcStart="0s" tcFormat="NDF" audioLayout="stereo" audioRate="48k"><spine>${spine}</spine></sequence></project></event></library></fcpxml>`;
}
