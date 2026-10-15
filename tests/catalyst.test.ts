import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import path from "node:path";
import os from "node:os";
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import {
  safeFile,
  secretScan,
  proposeChange,
  applyProposal,
} from "../apps/server/catalyst.ts";
import { correlate } from "../apps/server/media.ts";
const hash = (s: string) => createHash("sha256").update(s).digest("hex");
test("source access rejects traversal, private files, dependencies and symlinks", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sf-path-"));
  try {
    await fs.mkdir(path.join(dir, "apps"));
    await fs.writeFile(path.join(dir, "apps", "okay.ts"), "ok");
    await fs.symlink("/etc/passwd", path.join(dir, "apps", "link.ts"));
    assert.equal(
      await safeFile("apps/okay.ts", dir),
      path.join(dir, "apps", "okay.ts"),
    );
    for (const name of [
      "../README.md",
      "/etc/passwd",
      ".env",
      "apps/../README.md",
      "apps/node_modules/key.ts",
      "apps/link.ts",
      ".catalyst/private/proposals/a.json",
    ])
      await assert.rejects(() => safeFile(name, dir));
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
test("secret scanner blocks likely keys and private key material without blocking env examples", () => {
  assert.throws(() => secretScan("sk-proj-" + "a".repeat(40)), /secret/);
  assert.throws(() => secretScan("-----BEGIN PRIVATE KEY-----"), /secret/);
  assert.doesNotThrow(() =>
    secretScan("OPENAI_API_KEY=\nAuthorization: process.env.OPENAI_API_KEY"),
  );
});
test("a reviewed edit makes a real scoped Git commit, preserves other staged work and refuses stale undo", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sf-git-"));
  const git = (...args: string[]) =>
    execFileSync("git", args, { cwd: dir, encoding: "utf8" }).trim();
  try {
    git("init", "-q");
    git("config", "user.name", "Storyfold test");
    git("config", "user.email", "test@example.invalid");
    await fs.writeFile(path.join(dir, "README.md"), "Before\n");
    await fs.writeFile(path.join(dir, "other.txt"), "original\n");
    git("add", ".");
    git("commit", "-qm", "baseline");
    await fs.writeFile(path.join(dir, "other.txt"), "staged user work\n");
    git("add", "other.txt");
    const store = path.join(dir, "store");
    const p = await proposeChange(
      {
        path: "README.md",
        after: "After\n",
        summary: "Update welcome",
        hash: hash("Before\n"),
      },
      dir,
      store,
    );
    assert.equal(git("log", "-1", "--format=%s"), "baseline");
    const applied = await applyProposal(p.id, dir, store);
    assert.equal(applied.status, "applied");
    assert.equal(
      await fs.readFile(path.join(dir, "README.md"), "utf8"),
      "After\n",
    );
    assert.equal(git("show", "HEAD:other.txt"), "original");
    assert.equal(git("diff", "--cached", "--name-only"), "other.txt");
    const undo = await proposeChange(
      {
        path: "README.md",
        after: p.before,
        hash: hash(p.after),
        summary: "Undo welcome",
      },
      dir,
      store,
    );
    await applyProposal(undo.id, dir, store);
    assert.equal(
      await fs.readFile(path.join(dir, "README.md"), "utf8"),
      "Before\n",
    );
    const stale = await proposeChange(
      {
        path: "README.md",
        after: "later\n",
        hash: hash("Before\n"),
        summary: "Later",
      },
      dir,
      store,
    );
    await fs.writeFile(path.join(dir, "README.md"), "intervening edit\n");
    await assert.rejects(
      () => applyProposal(stale.id, dir, store),
      /changed after/,
    );
    assert.equal(
      await fs.readFile(path.join(dir, "README.md"), "utf8"),
      "intervening edit\n",
    );
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});
test("audio correlation finds a known offset and rejects silent confidence", () => {
  const a = Array.from(
    { length: 1500 },
    (_, i) => Math.sin(i * 0.037) ** 2 + Math.sin(i * i * 0.021) ** 2,
  );
  const b = [...Array(73).fill(0), ...a];
  const result = correlate(a, b);
  assert.equal(result.offset, 0.73);
  assert.ok(result.confidence > 0.99);
  assert.ok(
    correlate(Array(1500).fill(0), Array(1500).fill(0)).confidence < 0.55,
  );
});
