import test from "node:test";
import assert from "node:assert/strict";
import { applyProjectEdit } from "../apps/server/model.ts";
import { project } from "./fixtures.ts";
test("project editing preserves fixed speech timestamps and original source paths", () => {
  const p = project();
  const next = applyProjectEdit(p, {
    ...p,
    sentences: p.sentences.map((s) => ({
      ...s,
      start: 999,
      words: [],
      in: s.in + 0.5,
    })),
    cameras: [{ sourcePath: "/etc/passwd" }],
  });
  assert.equal(next.revision, 1);
  assert.equal(next.sentences[0].start, 1);
  assert.equal(next.cameras[0].sourcePath, "source-A.mp4");
  assert.equal(next.sentences[0].in, 1.5);
});
test("stale versions, invalid cuts, unknown folders and duplicated sentence IDs are rejected", () => {
  const p = project();
  assert.throws(
    () => applyProjectEdit(p, { ...p, revision: 3 }),
    /another window/,
  );
  assert.throws(
    () =>
      applyProjectEdit(p, {
        ...p,
        sentences: [...p.sentences.slice(1), p.sentences[1]],
      }),
    /exactly once/,
  );
  for (const patch of [
    { in: -1 },
    { in: 3.9, out: 4 },
    { topicId: "missing" },
    { cam: "C" },
  ])
    assert.throws(() =>
      applyProjectEdit(p, {
        ...p,
        sentences: p.sentences.map((s, i) =>
          i === 0 ? { ...s, ...patch } : s,
        ),
      }),
    );
});
