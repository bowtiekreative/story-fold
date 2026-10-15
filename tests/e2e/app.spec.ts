import { test, expect } from "@playwright/test";
import { promises as fs } from "node:fs";
import path from "node:path";
test("welcome, sample processing, editing, autosave, playback and export work together", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "The story is in there." }),
  ).toBeVisible();
  await page.screenshot({ path: "test-results/welcome.png", fullPage: true });
  await page.getByRole("button", { name: /Just looking/ }).click();
  await expect(page.getByText("Listening to every word.")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Story timeline" }),
  ).toBeVisible({ timeout: 60000 });
  await expect(
    page.getByText("Synthetic sample", { exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Start with every topic" }).click();
  await expect(page.getByText("8 clips ·")).toBeVisible();
  await page
    .getByRole("checkbox", { name: "L-cut into the next sentence" })
    .check();
  await page.getByLabel("Trim in", { exact: true }).fill("0.65");
  await expect(
    page.getByText("Saved on this computer", { exact: true }),
  ).toBeVisible({ timeout: 10000 });
  await page.reload();
  await expect(
    page.getByRole("checkbox", { name: "L-cut into the next sentence" }),
  ).toBeChecked();
  await expect(page.getByLabel("Trim in", { exact: true })).toHaveValue("0.65");
  await page.getByRole("button", { name: "Play story", exact: true }).click();
  await expect(
    page.getByRole("button", { name: "Pause story", exact: true }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Pause story", exact: true }).click();
  await page.screenshot({ path: "test-results/editor.png", fullPage: true });
  await page.getByRole("button", { name: "Export", exact: true }).click();
  await page.getByRole("button", { name: "Export story", exact: true }).click();
  await expect(
    page.getByRole("link", { name: "Download your video" }),
  ).toBeVisible({ timeout: 60000 });
  const url = await page
    .getByRole("link", { name: "Download your video" })
    .getAttribute("href");
  const response = await page.request.get(url!);
  expect(response.ok()).toBeTruthy();
  expect(response.headers()["content-type"]).toContain("video/mp4");
  expect(errors).toEqual([]);
});
test("Catalyst shows live tasks, real source review, pause controls and responsive previews", async ({
  page,
}) => {
  await page.goto("/catalyst");
  await expect(
    page.getByRole("heading", { name: "A clear view of what’s taking shape." }),
  ).toBeVisible();
  await expect(
    page.getByText("Connecting to the local service…"),
  ).not.toBeVisible();
  await page.screenshot({ path: "test-results/catalyst.png", fullPage: true });
  await page.getByRole("button", { name: "Pause pipeline" }).click();
  await expect(
    page.getByRole("button", { name: "Resume pipeline" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Build app", exact: true }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "Resume pipeline" }).click();
  await page.getByRole("button", { name: "Edit the app’s words" }).click();
  await expect(page.getByLabel("Source file editor")).toContainText("headline");
  const editor = page.getByLabel("Source file editor");
  const original = await editor.inputValue();
  await editor.fill(
    original.replace("The story is in there.", "There is a story here."),
  );
  await page
    .getByRole("button", { name: "Review change", exact: true })
    .click();
  await expect(
    page.getByRole("heading", { name: "See it before you save it." }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Apply & commit" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Discard proposal" }).click();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Tasks & roadmap" })
    .click();
  await expect(
    page.getByRole("heading", { name: "Now", exact: true }),
  ).toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Preview", exact: true })
    .click();
  await expect(
    page
      .frameLocator("iframe")
      .getByRole("heading", { name: "The story is in there." }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Phone", exact: true }).click();
  await expect(page.locator(".preview-device")).toHaveCSS("max-width", "390px");
});
test("phone layouts fit their viewport and expose the key controls", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await expect(
    page.getByRole("button", { name: "Choose your footage" }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "test-results/welcome-phone.png",
    fullPage: true,
  });
  await page.goto("/catalyst");
  await expect(
    page.getByRole("button", { name: "Pause pipeline" }),
  ).toBeVisible();
  expect(
    await page.evaluate(() => document.documentElement.scrollWidth),
  ).toBeLessThanOrEqual(390);
  await page.screenshot({
    path: "test-results/catalyst-phone.png",
    fullPage: true,
  });
});
test("a video upload with a timed transcript stays local and stale concurrent saves are rejected", async ({
  request,
}) => {
  const session = await request.get("/api/session");
  const { token } = await session.json();
  const headers = { "X-Storyfold-Token": token };
  const projects = await request.get("/api/projects").then((r) => r.json());
  const sample = projects.find(
    (p: { sample: boolean; status: string }) =>
      p.sample && p.status === "ready",
  );
  expect(sample).toBeTruthy();
  const projectDir = path.join(process.cwd(), "data", "projects", sample.id);
  const uploaded = await request.post("/api/projects", {
    headers,
    multipart: {
      name: "Upload acceptance fixture",
      grouping: "manual",
      files: {
        name: "interview.mp4",
        mimeType: "video/mp4",
        buffer: await fs.readFile(path.join(projectDir, "source-A.mp4")),
      },
      transcript: {
        name: "transcript.json",
        mimeType: "application/json",
        buffer: Buffer.from(
          JSON.stringify({
            words: [
              { text: "A timed", start: 1, end: 1.8 },
              { text: "sentence.", start: 1.9, end: 3 },
            ],
          }),
        ),
      },
    },
  });
  expect(uploaded.status()).toBe(202);
  const { projectId } = await uploaded.json();
  let project: import("../../packages/timeline/index.ts").Project;
  await expect
    .poll(
      async () => {
        project = await request
          .get("/api/projects/" + projectId)
          .then((r) => r.json());
        return project.status;
      },
      { timeout: 30000 },
    )
    .toBe("ready");
  expect(project!.grouping).toBe("Manual topics");
  expect(project!.sentences).toHaveLength(1);
  const outcomes = await Promise.all([
    request.patch("/api/projects/" + projectId, {
      headers,
      data: { ...project!, name: "First writer" },
    }),
    request.patch("/api/projects/" + projectId, {
      headers,
      data: { ...project!, name: "Second writer" },
    }),
  ]);
  expect(outcomes.map((r) => r.status()).sort()).toEqual([200, 409]);
  expect(
    (
      await request.patch("/api/projects/" + projectId, { data: project! })
    ).status(),
  ).toBe(403);
  expect(
    (
      await request.get(
        "/api/catalyst/file?path=" + encodeURIComponent("../.env"),
      )
    ).status(),
  ).toBe(400);
});
test("a reviewed edit can be applied and undone through real Git history", async ({
  page,
}) => {
  await page.goto("/catalyst");
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Code", exact: true })
    .click();
  await page
    .getByRole("button", { name: "docs/demos/REVIEW-CHECK.md", exact: true })
    .click();
  const editor = page.getByLabel("Source file editor");
  await expect(editor).toBeVisible();
  const original = await editor.inputValue();
  await editor.fill(original + "\nTemporary browser acceptance edit.\n");
  await page
    .getByLabel("What does this change do?")
    .fill("Verify Catalyst browser save");
  await page
    .getByRole("button", { name: "Review change", exact: true })
    .click();
  await page.getByRole("button", { name: "Apply & commit" }).click();
  await expect(
    page.getByRole("heading", { name: "See it before you save it." }),
  ).not.toBeVisible();
  await expect(editor).toHaveValue(
    original + "\nTemporary browser acceptance edit.\n",
  );
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "History", exact: true })
    .click();
  const row = page
    .locator(".review-row")
    .filter({
      has: page.getByText("Verify Catalyst browser save", { exact: true }),
    })
    .first();
  await row.getByRole("button", { name: "Review undo" }).click();
  await page.getByRole("button", { name: "Apply & commit" }).click();
  await expect(
    page.getByRole("heading", { name: "See it before you save it." }),
  ).not.toBeVisible();
  await page
    .getByRole("navigation")
    .getByRole("button", { name: "Code", exact: true })
    .click();
  await expect(editor).toHaveValue(original);
});
