import test from "node:test";
import assert from "node:assert/strict";
import { withProjectWrite } from "../apps/server/storage.ts";
test("project writes serialize revision checks so simultaneous stale edits cannot overwrite each other", async () => {
  let revision = 0;
  const edit = (expected: number) =>
    withProjectWrite("race-fixture", async () => {
      assert.equal(revision, expected, "stale revision");
      await new Promise((resolve) => setTimeout(resolve, 10));
      revision++;
      return revision;
    });
  const results = await Promise.allSettled([edit(0), edit(0)]);
  assert.equal(results[0].status, "fulfilled");
  assert.equal(results[1].status, "rejected");
  assert.equal(revision, 1);
});
