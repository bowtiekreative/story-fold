import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";
import FFT from "fft.js";
import {
  sequence,
  splitSentences,
  buildSrt,
  buildEdl,
  buildFcpxml,
  type Project,
  type Word,
  type Accent,
} from "../../packages/timeline/index.ts";
import { projectDir, saveProject, readJson, writeJson } from "./storage.ts";
import { wordSchema } from "./model.ts";
import { classifyTopics, topicSettings, TopicProviderError } from "./topics.ts";
import { speechStatus, speechWords, whisperPaths } from "./whisper.ts";

export async function command(
  bin: string,
  args: string[],
  onOutput?: (text: string) => void,
  options: { timeoutMs?: number; env?: NodeJS.ProcessEnv } = {},
): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, {
      stdio: ["ignore", "pipe", "pipe"],
      windowsHide: true,
      env: options.env ?? process.env,
    });
    const timeout = options.timeoutMs
      ? setTimeout(() => {
          child.kill("SIGKILL");
          reject(
            new Error(
              `${path.basename(bin)} took too long. Try a shorter clip or resume processing.`,
            ),
          );
        }, options.timeoutMs)
      : undefined;
    const chunks: Buffer[] = [];
    let error = "";
    child.stdout.on("data", (b: Buffer) => {
      chunks.push(b);
      onOutput?.(b.toString());
    });
    child.stderr.on("data", (b: Buffer) => {
      error = (error + b.toString()).slice(-4000);
    });
    child.on("error", () =>
      reject(
        new Error(
          `${bin} could not start. Install it and make sure it is on PATH.`,
        ),
      ),
    );
    child.on("close", (code) => {
      clearTimeout(timeout);
      if (code === 0) resolve(Buffer.concat(chunks));
      else
        reject(
          new Error(`${path.basename(bin)} failed: ${error.slice(-1500)}`),
        );
    });
  });
}
const ff = (args: string[], onOutput?: (text: string) => void) =>
  command(
    "ffmpeg",
    ["-hide_banner", "-loglevel", "error", "-y", ...args],
    onOutput,
  );
export async function probe(file: string) {
  const info = JSON.parse(
    (
      await command("ffprobe", [
        "-v",
        "error",
        "-show_format",
        "-show_streams",
        "-of",
        "json",
        file,
      ])
    ).toString(),
  ) as {
    format: { duration: string };
    streams: { codec_type: string; width?: number; height?: number }[];
  };
  const duration = Number(info.format.duration);
  if (
    !Number.isFinite(duration) ||
    duration <= 0 ||
    duration > 7200 ||
    !info.streams.some((s) => s.codec_type === "video")
  )
    throw new Error("Upload a video between 0 and 2 hours long.");
  if (!info.streams.some((s) => s.codec_type === "audio"))
    throw new Error("The video needs an audio track.");
  return {
    duration,
    width: info.streams.find((s) => s.codec_type === "video")!.width ?? 1280,
    height: info.streams.find((s) => s.codec_type === "video")!.height ?? 720,
  };
}
export function audioEnvelope(samples: Buffer, window = 10) {
  const result: number[] = [];
  for (let i = 0; i + window * 4 <= samples.length; i += window * 4) {
    let sum = 0;
    for (let j = 0; j < window; j++) sum += samples.readFloatLE(i + j * 4) ** 2;
    result.push(Math.sqrt(sum / window));
  }
  return result;
}
export function correlate(a: number[], b: number[], rate = 100) {
  if (!a.length || !b.length) return { offset: 0, confidence: 0 };
  const size = 2 ** Math.ceil(Math.log2(a.length + b.length)),
    fft = new FFT(size);
  const left = Array(size).fill(0),
    right = Array(size).fill(0);
  a.forEach((v, i) => {
    left[a.length - 1 - i] = v;
  });
  b.forEach((v, i) => {
    right[i] = v;
  });
  const fa = fft.createComplexArray(),
    fb = fft.createComplexArray(),
    product = fft.createComplexArray(),
    convolution = fft.createComplexArray();
  fft.realTransform(fa, left);
  fft.completeSpectrum(fa);
  fft.realTransform(fb, right);
  fft.completeSpectrum(fb);
  for (let i = 0; i < product.length; i += 2) {
    product[i] = fa[i] * fb[i] - fa[i + 1] * fb[i + 1];
    product[i + 1] = fa[i] * fb[i + 1] + fa[i + 1] * fb[i];
  }
  fft.inverseTransform(convolution, product);
  const prefix = (values: number[], squared = false) => {
    const out = [0];
    values.forEach((v) => out.push(out.at(-1)! + (squared ? v * v : v)));
    return out;
  };
  const pa = prefix(a),
    pb = prefix(b),
    pqa = prefix(a, true),
    pqb = prefix(b, true);
  let lag = 0,
    peak = 0;
  const range = Math.min(120 * rate, Math.max(a.length, b.length) - 1),
    minOverlap = Math.min(rate * 5, Math.min(a.length, b.length) / 2);
  for (let l = -range; l <= range; l++) {
    const lo = Math.max(0, -l),
      hi = Math.min(a.length, b.length - l),
      n = hi - lo;
    if (n < minOverlap) continue;
    const sa = pa[hi] - pa[lo],
      sb = pb[hi + l] - pb[lo + l],
      aa = pqa[hi] - pqa[lo] - (sa * sa) / n,
      bb = pqb[hi + l] - pqb[lo + l] - (sb * sb) / n;
    if (aa < 1e-10 || bb < 1e-10) continue;
    const score =
      (convolution[2 * (a.length - 1 + l)] - (sa * sb) / n) /
      Math.sqrt(aa * bb);
    if (score > peak) {
      peak = score;
      lag = l;
    }
  }
  return { offset: lag / rate, confidence: peak };
}
async function envelope(file: string) {
  return audioEnvelope(
    await command("ffmpeg", [
      "-v",
      "error",
      "-i",
      file,
      "-t",
      "300",
      "-vn",
      "-ar",
      "1000",
      "-ac",
      "1",
      "-f",
      "f32le",
      "pipe:1",
    ]),
  );
}
const accents: Accent[] = ["violet", "coral", "teal", "amber", "sky", "lime"];
async function transcribe(
  project: Project,
  changed: (detail: string, pct: number) => Promise<void>,
): Promise<Word[]> {
  const status = await speechStatus();
  if (!status.transcription)
    throw new Error(
      "Automatic Whisper transcription needs setup. Run npm run setup:whisper in the project terminal, then resume processing.",
    );
  const dir = projectDir(project.id);
  const words: Word[] = [];
  for (let start = 0; start < project.cameras[0].duration; start += 599) {
    await changed(
      `${status.speechProvider}: transcribing from minute ${Math.floor(start / 60)} of ${Math.ceil(project.cameras[0].duration / 60)}`,
      30 + (20 * start) / project.cameras[0].duration,
    );
    const wav = path.join(dir, `audio-${start}.wav`);
    const checkpoint = path.join(dir, `transcript-${start}.json`);
    let result: unknown;
    if (await exists(checkpoint)) {
      result = await readJson(checkpoint);
    } else {
      await ff([
        "-ss",
        String(start),
        "-i",
        path.join(dir, project.cameras[0].sourcePath),
        "-t",
        "600",
        "-vn",
        "-ar",
        "16000",
        "-ac",
        "1",
        wav,
      ]);
      if (status.speechProvider === "Local Whisper") {
        const worker = whisperPaths();
        const output = await command(
          worker.python,
          [worker.worker, "--model", worker.model, "--audio", wav],
          undefined,
          { timeoutMs: 1800000, env: { ...process.env, HF_HUB_OFFLINE: "1" } },
        );
        result = JSON.parse(output.toString());
      } else {
        const form = new FormData();
        form.set(
          "file",
          new Blob([new Uint8Array(await fs.readFile(wav))], {
            type: "audio/wav",
          }),
          "interview.wav",
        );
        form.set("model", "whisper-1");
        form.set("response_format", "verbose_json");
        form.append("timestamp_granularities[]", "word");
        const response = await fetch(
          "https://api.openai.com/v1/audio/transcriptions",
          {
            method: "POST",
            headers: { Authorization: `Bearer ${process.env.OPENAI_API_KEY}` },
            body: form,
            signal: AbortSignal.timeout(300000),
          },
        );
        if (!response.ok)
          throw new Error(
            `Transcription provider returned ${response.status}. Check your server key, credits and audio format.`,
          );
        result = await response.json();
      }
      speechWords(result, start, project.cameras[0].duration);
      await writeJson(checkpoint, result);
    }
    for (const word of speechWords(
      result,
      start,
      project.cameras[0].duration,
    )) {
      if (
        word.start >= (words.at(-1)?.end ?? 0) - 0.03 &&
        word.end > word.start
      )
        words.push(word);
    }
  }
  if (!words.length)
    throw new Error(
      "Whisper found no speech. Check that camera A has audible dialogue, then try again.",
    );
  await changed(
    `${status.speechProvider}: ${words.length} words transcribed`,
    50,
  );
  return words;
}
export async function groupTopics(
  project: Project,
  changed?: (detail: string, fraction: number) => Promise<void>,
) {
  if (!topicSettings().topics) {
    project.topics = [
      {
        id: "interview",
        name: "Interview",
        accent: "violet",
        summary:
          "Create topic folders and move sentences to organize your transcript.",
      },
    ];
    project.grouping = "Manual topics";
    for (const sentence of project.sentences) sentence.topicId = "interview";
    project.warnings.push(
      "Configure a Claude or OpenAI key for automatic topic grouping. Your transcript is ready to organize manually.",
    );
    return;
  }
  try {
    const { topics, assignments } = await classifyTopics(
      project.sentences,
      changed,
    );
    project.topics = topics.map((topic, i) => ({
      ...topic,
      accent: accents[i % accents.length],
    }));
    for (const sentence of project.sentences)
      sentence.topicId = assignments.get(sentence.n)!;
    project.grouping = `${topicSettings().topicProvider} topic suggestions — review before editing`;
  } catch (error) {
    project.topics = [{ id: "interview", name: "Interview", accent: "violet" }];
    project.grouping = "Manual topics";
    for (const sentence of project.sentences) sentence.topicId = "interview";
    project.warnings.push(
      (error instanceof TopicProviderError
        ? error.message
        : "Topic classification failed validation.") +
        " Your transcript is intact; organize it manually.",
    );
  }
}
async function exists(file: string) {
  return fs.access(file).then(
    () => true,
    () => false,
  );
}
export type ProcessingInput = {
  words?: Word[];
  manual: boolean;
  sample?: boolean;
};
export async function processProject(
  project: Project,
  input: ProcessingInput,
  changed: () => void,
) {
  const dir = projectDir(project.id);
  async function progress(
    step: Project["status"],
    detail: string,
    pct: number,
  ) {
    project.status = step;
    project.progress.push({ step, detail, pct, at: new Date().toISOString() });
    await saveProject(project);
    changed();
  }
  try {
    await progress(
      "uploading",
      "Checking your footage and making playback copies",
      5,
    );
    for (const camera of project.cameras.filter((c) => !c.virtual)) {
      const source = path.join(dir, camera.sourcePath);
      const meta = await probe(source);
      camera.duration = meta.duration;
      const proxy = path.join(dir, camera.proxyPath);
      if (!(await exists(proxy)))
        await ff([
          "-i",
          source,
          "-vf",
          "scale='min(1280,iw)':-2",
          "-c:v",
          "libx264",
          "-preset",
          "veryfast",
          "-crf",
          "25",
          "-g",
          "13",
          "-keyint_min",
          "13",
          "-sc_threshold",
          "0",
          "-c:a",
          "aac",
          "-movflags",
          "+faststart",
          proxy,
        ]);
    }
    if (project.cameras.length === 1) {
      const master = project.cameras[0];
      project.cameras.push(
        {
          ...master,
          id: "B",
          name: "Close-up",
          virtual: { scale: 1.65, x: 0.5, y: 0.45 },
        },
        {
          ...master,
          id: "C",
          name: "Side angle",
          virtual: { scale: 1.3, x: 0.3, y: 0.5 },
        },
      );
    }
    const peaksFile = path.join(dir, "peaks.json");
    if (!(await exists(peaksFile))) {
      const pcm = await command("ffmpeg", [
        "-v",
        "error",
        "-i",
        path.join(dir, project.cameras[0].sourcePath),
        "-vn",
        "-ac",
        "1",
        "-ar",
        "1000",
        "-f",
        "f32le",
        "pipe:1",
      ]);
      const peaks = audioEnvelope(pcm, 50);
      const max = Math.max(
        0.0001,
        ...peaks.slice(0, 10000),
        ...peaks.slice(-10000),
      );
      await writeJson(peaksFile, {
        rate: 20,
        peaks: peaks.map((p) => Math.min(1, p / max)),
      });
    }
    await progress(
      "transcribing",
      input.sample
        ? "Loading the labeled sample transcript"
        : input.words
          ? "Reading your timed transcript"
          : "Transcribing speech with word timestamps",
      30,
    );
    const wordFile = path.join(dir, "words.json");
    const words = (await exists(wordFile))
      ? await readJson<Word[]>(wordFile)
      : (input.words ??
        (await transcribe(project, (detail, pct) =>
          progress("transcribing", detail, pct),
        )));
    wordSchema.array().min(1).max(200000).parse(words);
    await writeJson(wordFile, words);
    await progress("segmenting", "Finding sentence boundaries", 55);
    project.sentences = splitSentences(words, project.cameras[0].duration);
    await progress(
      "syncing",
      project.cameras.some((c) => c.virtual)
        ? "Creating two reframed angles from camera A"
        : "Matching the camera audio",
      68,
    );
    if (project.cameras.filter((c) => !c.virtual).length > 1) {
      const master = await envelope(
        path.join(dir, project.cameras[0].sourcePath),
      );
      for (const cam of project.cameras.slice(1).filter((c) => !c.virtual)) {
        const result = correlate(
          master,
          await envelope(path.join(dir, cam.sourcePath)),
        );
        cam.offset = result.offset;
        if (result.confidence < 0.55)
          project.warnings.push(
            `Camera ${cam.id}: audio sync is uncertain. Check and adjust the offset before editing.`,
          );
      }
    }
    await progress(
      "grouping",
      input.manual
        ? "Keeping your transcript ready for topic folders"
        : "Suggesting topic folders",
      82,
    );
    if (input.sample) {
      project.topics = sampleTopics;
      project.sentences.forEach((s, i) => {
        s.topicId = sampleLines[i].topic;
      });
      project.grouping = "Sample folders — supplied with the demo";
    } else if (input.manual) {
      project.topics = [
        { id: "interview", name: "Interview", accent: "violet" },
      ];
      project.grouping = "Manual topics";
    } else
      await groupTopics(project, (detail, fraction) =>
        progress("grouping", detail, 82 + 14 * fraction),
      );
    const seen = new Set<string>();
    let k = 0;
    project.sentences.forEach((s) => {
      s.cam = !seen.has(s.topicId)
        ? "A"
        : project.cameras[++k % project.cameras.length].id;
      seen.add(s.topicId);
    });
    project.timeline = [];
    project.error = undefined;
    project.revision++;
    await progress(
      "ready",
      `${project.sentences.length} sentences in ${project.topics.length} topic folders`,
      100,
    );
  } catch (error) {
    project.error =
      error instanceof Error ? error.message : "Processing failed.";
    await progress("error", project.error, 0);
  }
}
export const sampleTopics: Project["topics"] = [
  { id: "growing", name: "Growing up", accent: "amber" },
  { id: "mom", name: "Mom", accent: "coral" },
  { id: "bakery", name: "The bakery", accent: "teal" },
  { id: "advice", name: "Advice", accent: "violet" },
];
export const sampleLines = [
  { text: "I grew up above a bakery on Delancey Street.", topic: "growing" },
  {
    text: "Every morning the whole building smelled like rye bread.",
    topic: "bakery",
  },
  { text: "My mom was up at four, every single day.", topic: "mom" },
  { text: "She never once complained about it.", topic: "mom" },
  { text: "We did not have much, but we never felt poor.", topic: "growing" },
  {
    text: "The whole block showed up with buckets and brooms.",
    topic: "bakery",
  },
  { text: "That is when I understood what community means.", topic: "advice" },
  { text: "If you love someone, show up early.", topic: "advice" },
];
export async function createSample(project: Project): Promise<Word[]> {
  const dir = projectDir(project.id);
  await fs.mkdir(dir, { recursive: true });
  // shortcut: synthetic test-pattern footage, replace with Ryan's interview for speech and sync acceptance.
  await ff([
    "-f",
    "lavfi",
    "-i",
    "testsrc2=size=960x540:rate=25",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=220:sample_rate=48000",
    "-t",
    "36",
    "-vf",
    "drawbox=x=0:y=ih-90:w=iw:h=90:color=black@0.8:t=fill,drawtext=text='STORYFOLD  -  SYNTHETIC SAMPLE':fontcolor=white:fontsize=28:x=(w-text_w)/2:y=h-65",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-c:a",
    "aac",
    "-af",
    "volume=0.03",
    "-shortest",
    path.join(dir, "source-A.mp4"),
  ]);
  return sampleLines.flatMap((line, i) => {
    const tokens = line.text.split(" ");
    const step = 3.6 / tokens.length;
    return tokens.map((text, j) => ({
      text,
      start: i * 4.5 + 0.3 + j * step,
      end: i * 4.5 + 0.3 + (j + 0.9) * step,
    }));
  });
}
export interface ExportJob {
  id: string;
  projectId: string;
  status: "queued" | "rendering" | "done" | "error";
  pct: number;
  detail: string;
  files: string[];
  error?: string;
  revision: number;
}
const assTime = (t: number) => {
  const cs = Math.round(t * 100);
  return `${Math.floor(cs / 360000)}:${String(Math.floor(cs / 6000) % 60).padStart(2, "0")}:${String(Math.floor(cs / 100) % 60).padStart(2, "0")}.${String(cs % 100).padStart(2, "0")}`;
};
export async function renderExport(
  project: Project,
  job: ExportJob,
  changed: () => void,
) {
  const dir = projectDir(project.id),
    outDir = path.join(dir, "exports", job.id);
  await fs.mkdir(outDir, { recursive: true });
  const persist = async () => {
    await writeJson(path.join(outDir, "job.json"), job);
    changed();
  };
  try {
    const sq = sequence(project);
    if (!sq.clips.length)
      throw new Error("Add a topic or sentence to the story before exporting.");
    job.status = "rendering";
    job.detail = "Preparing your cuts";
    await persist();
    await fs.writeFile(path.join(outDir, "story.srt"), buildSrt(project));
    await fs.writeFile(path.join(outDir, "story.edl"), buildEdl(project));
    await fs.writeFile(path.join(outDir, "story.fcpxml"), buildFcpxml(project));
    const [w, h] =
      project.exportSettings.ratio === "9:16"
        ? [720, 1280]
        : project.exportSettings.ratio === "1:1"
          ? [720, 720]
          : [1280, 720];
    const filter: string[] = [];
    const videos: string[] = [],
      audios: string[] = [];
    const inputs = project.cameras.flatMap((c) => [
      "-i",
      path.join(dir, c.sourcePath),
    ]);
    sq.clips.forEach((c, i) => {
      const cam = project.cameras.find((x) => x.id === c.sentence.cam)!;
      const ci = project.cameras.indexOf(cam);
      const scale = cam.virtual?.scale ?? 1,
        x = cam.virtual?.x ?? 0.5,
        y = cam.virtual?.y ?? 0.5;
      const from = c.sentence.in + cam.offset,
        frames = Math.max(
          1,
          Math.round(c.vEnd * 25) - Math.round(c.vStart * 25),
        ),
        length = frames / 25;
      if (from < 0 || from >= cam.duration)
        throw new Error(
          `Camera ${cam.id} has no footage at this cut. Adjust its sync offset or choose another camera.`,
        );
      filter.push(
        `[${ci}:v]trim=start=${from}:duration=${length},setpts=PTS-STARTPTS,crop=iw/${scale}:ih/${scale}:(iw-ow)*${x}:(ih-oh)*${y},scale=${w}:${h}:force_original_aspect_ratio=increase,crop=${w}:${h},setsar=1,fps=25,tpad=stop_mode=clone:stop_duration=${length},trim=end_frame=${frames},setpts=PTS-STARTPTS[v${i}]`,
      );
      filter.push(
        `[0:a]atrim=start=${c.sentence.in}:duration=${c.dur},asetpts=PTS-STARTPTS,aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo,apad,atrim=duration=${c.dur},afade=t=in:d=0.005,afade=t=out:st=${Math.max(0, c.dur - 0.005)}:d=0.005[a${i}]`,
      );
      videos.push(`[v${i}]`);
      audios.push(`[a${i}]`);
    });
    filter.push(
      `${videos.join("")}concat=n=${videos.length}:v=1:a=0[vjoined]`,
      `${audios.join("")}concat=n=${audios.length}:v=0:a=1[aout]`,
    );
    let videoLabel = "vjoined";
    const subtitles = [
      "[Script Info]",
      "ScriptType: v4.00+",
      `PlayResX: ${w}`,
      `PlayResY: ${h}`,
      "[V4+ Styles]",
      "Format: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding",
      `Style: Default,DejaVu Sans,${Math.round(w * 0.031)},&H00FFFFFF,&H00FFFFFF,&H00222222,&H99000000,0,0,0,0,100,100,0,0,3,2,0,2,40,40,${Math.round(h * 0.07)},1`,
      "[Events]",
      "Format: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text",
    ];
    for (const c of sq.clips) {
      const tokens = c.sentence.text.replace(/[{}\\\r\n]/g, "").split(" ");
      const half = Math.ceil(tokens.length / 2);
      const text =
        tokens.length > 7
          ? tokens.slice(0, half).join(" ") +
            "\\N" +
            tokens.slice(half).join(" ")
          : tokens.join(" ");
      subtitles.push(
        `Dialogue: 0,${assTime(c.aStart)},${assTime(c.aEnd)},Default,,0,0,0,,${text}`,
      );
    }
    const subtitlePath = path.join(outDir, "captions.ass");
    await fs.writeFile(subtitlePath, subtitles.join("\n"));
    if (project.exportSettings.captions) {
      const escaped = subtitlePath
        .replace(/\\/g, "/")
        .replace(/:/g, "\\:")
        .replace(/'/g, "'\\''");
      filter.push(`[vjoined]ass=filename='${escaped}'[vout]`);
      videoLabel = "vout";
    }
    const chapters = [";FFMETADATA1"];
    const chapterLines: string[] = [];
    if (project.exportSettings.chapters)
      sq.spans.forEach((span, i) => {
        if (!span.count) return;
        const item = project.timeline[i];
        const title =
          item.kind === "group"
            ? project.topics.find((t) => t.id === item.topicId)!.name
            : project.sentences.find((s) => s.id === item.sentenceId)!.text;
        chapters.push(
          "[CHAPTER]",
          "TIMEBASE=1/1000",
          `START=${Math.round(span.start * 1000)}`,
          `END=${Math.round(span.end * 1000)}`,
          `title=${title.replace(/[=;#\\\n]/g, " ")}`,
        );
        chapterLines.push(
          `${Math.floor(span.start / 60)}:${String(Math.floor(span.start) % 60).padStart(2, "0")} ${title}`,
        );
      });
    const metadata = path.join(outDir, "chapters.ffmeta");
    await fs.writeFile(metadata, chapters.join("\n"));
    await fs.writeFile(
      path.join(outDir, "chapters.txt"),
      chapterLines.join("\n"),
    );
    const script = path.join(outDir, "filters.txt");
    await fs.writeFile(script, filter.join(";\n"));
    job.detail = "Rendering video, angles and L-cuts";
    await persist();
    let last = 0;
    await ff(
      [
        ...inputs,
        "-f",
        "ffmetadata",
        "-i",
        metadata,
        "-filter_complex_threads",
        "1",
        "-filter_complex_script",
        script,
        "-map",
        `[${videoLabel}]`,
        "-map",
        "[aout]",
        "-map_metadata",
        String(project.cameras.length),
        "-map_chapters",
        String(project.cameras.length),
        "-c:v",
        "libx264",
        "-preset",
        "veryfast",
        "-crf",
        "22",
        "-pix_fmt",
        "yuv420p",
        "-c:a",
        "aac",
        "-b:a",
        "192k",
        "-t",
        String(sq.total),
        "-movflags",
        "+faststart",
        "-progress",
        "pipe:1",
        path.join(outDir, "story.mp4"),
      ],
      (output) => {
        const m = output.match(/out_time_us=(\d+)/);
        if (m) {
          job.pct = Math.min(
            99,
            Math.round((Number(m[1]) / 1e6 / sq.total) * 100),
          );
          if (Date.now() - last > 700) {
            last = Date.now();
            void persist();
          }
        }
      },
    );
    job.status = "done";
    job.pct = 100;
    job.detail = "Your story is ready";
    job.files = [
      "story.mp4",
      "story.edl",
      "story.fcpxml",
      "story.srt",
      "chapters.txt",
    ];
    await persist();
  } catch (error) {
    job.status = "error";
    job.error = error instanceof Error ? error.message : "Export failed";
    job.detail = job.error;
    await persist();
  }
}
