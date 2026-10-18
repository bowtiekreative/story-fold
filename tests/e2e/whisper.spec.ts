import { test, expect } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";
import type { Project } from "../../packages/timeline/index.ts";
import { command } from "../../apps/server/media.ts";

test("local Whisper turns a speech video into timed sentence clips without JSON", async ({
  page,
  request,
}) => {
  test.skip(
    process.env.STORYFOLD_TEST_WHISPER !== "1",
    "Opt-in check requires local Whisper and FFmpeg with its flite filter.",
  );
  test.setTimeout(240000);
  const fixture = path.join(
    process.cwd(),
    "data/validation/whisper-speech.mp4",
  );
  const health = await request.get("/api/health").then((r) => r.json());
  expect(health.speechProvider).toBe("Local Whisper");
  await fs.mkdir(path.dirname(fixture), { recursive: true });
  await command("ffmpeg", [
    "-hide_banner",
    "-loglevel",
    "error",
    "-y",
    "-f",
    "lavfi",
    "-i",
    "color=c=0x29243b:s=320x180:r=25",
    "-f",
    "lavfi",
    "-i",
    "flite=text='Welcome to the bakery. Every morning we make fresh bread for our neighbors. My mother taught me to be patient. We started with one small oven. Today our family works together. The best part of this job is seeing people smile.':voice=slt",
    "-shortest",
    "-c:v",
    "libx264",
    "-preset",
    "ultrafast",
    "-c:a",
    "aac",
    fixture,
  ]);
  await page.goto("/");
  await expect(
    page.getByText("Local Whisper transcribes your video automatically."),
  ).toBeVisible();
  await page.locator('input[type="file"][multiple]').setInputFiles(fixture);
  await page
    .getByLabel("Interview name")
    .fill("Whisper validation · synthetic speech");
  await page.getByLabel("Organize topics myself").check();
  await expect(page.getByLabel(/Timed transcript JSON/)).toHaveValue("");
  await page
    .getByRole("button", { name: "Find my story", exact: true })
    .click();
  await expect(page).toHaveURL(/\?project=/);
  const id = new URL(page.url()).searchParams.get("project");
  let project: Project;
  await expect
    .poll(
      async () => {
        project = await request
          .get("/api/projects/" + id)
          .then((r) => r.json());
        return ["ready", "error"].includes(project.status);
      },
      { timeout: 180000 },
    )
    .toBe(true);
  expect(project!.status, project!.error).toBe("ready");
  expect(project!.grouping).toBe("Manual topics");
  const words = project!.sentences.flatMap((sentence) => sentence.words);
  const transcript = words
    .map((word) => word.text)
    .join(" ")
    .toLowerCase();
  expect(words.length).toBeGreaterThan(30);
  for (const keyword of ["bakery", "bread", "mother", "oven", "family"])
    expect(transcript).toContain(keyword);
  for (const [i, word] of words.entries()) {
    expect(Number.isFinite(word.start) && Number.isFinite(word.end)).toBe(true);
    expect(word.start).toBeGreaterThanOrEqual(i ? words[i - 1].start : 0);
    expect(word.end).toBeGreaterThan(word.start);
    expect(word.end).toBeLessThanOrEqual(project!.cameras[0].duration + 0.15);
  }
  await expect(
    page.getByRole("heading", { name: "Story timeline" }),
  ).toBeVisible();
  await page.screenshot({
    path: "test-results/whisper-editor.png",
    fullPage: true,
  });
  await fs.writeFile(
    path.join(process.cwd(), "docs/validation/whisper-smoke.json"),
    JSON.stringify(
      {
        at: new Date().toISOString(),
        fixture:
          "Locally synthesized speech with FFmpeg flite/slt; no private footage",
        speechProvider: health.speechProvider,
        projectId: id,
        sourceDuration: project!.cameras[0].duration,
        sentenceCount: project!.sentences.length,
        wordCount: words.length,
        grouping: project!.grouping,
        importedTranscript: false,
        words,
      },
      null,
      2,
    ) + "\n",
  );
});
