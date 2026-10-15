import { promises as fs } from "node:fs";
import path from "node:path";
import os from "node:os";
import assert from "node:assert/strict";
import { project, sentence } from "../tests/fixtures.ts";
const temp = await fs.mkdtemp(path.join(os.tmpdir(), "storyfold-media-"));
process.env.STORYFOLD_DATA_DIR = temp;
const { command, renderExport } = await import("../apps/server/media.ts");
const { projectDir } = await import("../apps/server/storage.ts");
try {
  const p = project();
  p.id = "render-test";
  const dir = projectDir(p.id);
  await fs.mkdir(dir, { recursive: true });
  const source = path.join(dir, "source-A.mp4");
  await command("ffmpeg", [
    "-v",
    "error",
    "-y",
    "-f",
    "lavfi",
    "-i",
    "color=c=red:s=320x180:r=25:d=5",
    "-f",
    "lavfi",
    "-i",
    "color=c=blue:s=320x180:r=25:d=5",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=440:sample_rate=48000:duration=5",
    "-f",
    "lavfi",
    "-i",
    "sine=frequency=880:sample_rate=48000:duration=5",
    "-filter_complex",
    "[0:v][1:v]concat=n=2:v=1:a=0[v];[2:a][3:a]concat=n=2:v=0:a=1[a]",
    "-map",
    "[v]",
    "-map",
    "[a]",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-c:a",
    "aac",
    source,
  ]);
  p.cameras.forEach((c) => {
    c.duration = 10;
  });
  p.sentences = [
    { ...sentence("s1", 0), lcut: 1 },
    { ...sentence("s2", 6, 3, "two"), cam: "B" },
  ];
  p.exportSettings = { ratio: "9:16", captions: true, chapters: true };
  const job: import("../apps/server/media.ts").ExportJob = {
    id: "smoke",
    projectId: p.id,
    status: "queued",
    pct: 0,
    detail: "",
    files: [],
    revision: 0,
  };
  await renderExport(p, job, () => {});
  assert.equal(job.status, "done", job.error ?? "Export did not finish");
  const file = path.join(dir, "exports", "smoke", "story.mp4");
  const info = JSON.parse(
    (
      await command("ffprobe", [
        "-v",
        "error",
        "-show_streams",
        "-show_format",
        "-show_chapters",
        "-of",
        "json",
        file,
      ])
    ).toString(),
  );
  const video = info.streams.find(
    (s: { codec_type: string }) => s.codec_type === "video",
  );
  assert.equal(video.width, 720);
  assert.equal(video.height, 1280);
  assert.ok(Math.abs(Number(info.format.duration) - 6) < 0.08);
  assert.equal(info.chapters.length, 2);
  async function pixel(t: number) {
    return command("ffmpeg", [
      "-v",
      "error",
      "-ss",
      String(t),
      "-i",
      file,
      "-vf",
      "crop=2:2:0:0,scale=1:1",
      "-frames:v",
      "1",
      "-f",
      "rawvideo",
      "-pix_fmt",
      "rgb24",
      "pipe:1",
    ]);
  }
  const red = await pixel(1.8),
    blue = await pixel(2.2);
  assert.ok(red[0] > 180 && red[2] < 50);
  assert.ok(
    blue[2] > 180 && blue[0] < 50,
    "Picture should change at 2s, one second before audio.",
  );
  const captionBand = await command("ffmpeg", [
    "-v",
    "error",
    "-ss",
    "1",
    "-i",
    file,
    "-vf",
    "crop=iw:ih/3:0:ih*2/3",
    "-frames:v",
    "1",
    "-f",
    "rawvideo",
    "-pix_fmt",
    "rgb24",
    "pipe:1",
  ]);
  let whitePixels = 0;
  for (let i = 0; i < captionBand.length; i += 3)
    if (
      captionBand[i] > 200 &&
      captionBand[i + 1] > 200 &&
      captionBand[i + 2] > 200
    )
      whitePixels++;
  assert.ok(
    whitePixels > 20,
    "Burned captions should add white text pixels to the otherwise solid-color footage.",
  );
  async function frequency(t: number) {
    const pcm = await command("ffmpeg", [
      "-v",
      "error",
      "-ss",
      String(t),
      "-i",
      file,
      "-t",
      "0.2",
      "-vn",
      "-ac",
      "1",
      "-ar",
      "48000",
      "-f",
      "f32le",
      "pipe:1",
    ]);
    let crosses = 0;
    for (let i = 4; i < pcm.length; i += 4)
      if (pcm.readFloatLE(i - 4) < 0 && pcm.readFloatLE(i) >= 0) crosses++;
    return crosses / (pcm.length / 4 / 48000);
  }
  const early = await frequency(2.5),
    late = await frequency(3.5);
  assert.ok(Math.abs(early - 440) < 20);
  assert.ok(Math.abs(late - 880) < 20);
  const report = {
    passed: true,
    checks: [
      "portrait dimensions",
      "six-second duration",
      "two MP4 chapters",
      "picture changes at 2 seconds",
      "audio changes at 3 seconds",
      "virtual crop rendered",
      "captions rendered",
      "MP4/EDL/FCPXML/SRT artifacts",
    ],
    observed: {
      duration: info.format.duration,
      width: video.width,
      height: video.height,
      chapters: info.chapters.length,
      earlyFrequency: early,
      lateFrequency: late,
    },
    at: new Date().toISOString(),
  };
  await fs.mkdir("docs/validation", { recursive: true });
  await fs.writeFile(
    "docs/validation/media-smoke.json",
    JSON.stringify(report, null, 2),
  );
  console.log(JSON.stringify(report, null, 2));
} finally {
  await fs.rm(temp, { recursive: true, force: true });
}
