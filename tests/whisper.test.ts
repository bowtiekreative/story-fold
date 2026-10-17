import test from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import {
  speechStatus,
  speechWords,
  whisperPaths,
} from "../apps/server/whisper.ts";

test("local Whisper readiness requires both completed setup and all model assets", async () => {
  const dir = await fs.mkdtemp(path.join(os.tmpdir(), "sf-whisper-"));
  try {
    assert.equal((await speechStatus(dir, {})).transcription, false);
    assert.equal(
      (await speechStatus(dir, { OPENAI_API_KEY: "example" })).speechProvider,
      "OpenAI Whisper",
    );
    const files = whisperPaths(dir);
    for (const file of [
      files.python,
      files.ready,
      path.join(files.model, "model.bin"),
      path.join(files.model, "config.json"),
      path.join(files.model, "tokenizer.json"),
    ]) {
      await fs.mkdir(path.dirname(file), { recursive: true });
      await fs.writeFile(file, "fixture");
    }
    assert.deepEqual(await speechStatus(dir, { OPENAI_API_KEY: "example" }), {
      transcription: true,
      speechProvider: "Local Whisper",
    });
    await fs.rm(files.ready);
    assert.equal((await speechStatus(dir, {})).transcription, false);
  } finally {
    await fs.rm(dir, { recursive: true, force: true });
  }
});

test("Whisper words preserve punctuation and gain source-clock timestamps", () => {
  assert.deepEqual(
    speechWords(
      {
        text: "Hello, world.",
        words: [
          { word: "Hello", start: 0.2, end: 0.7 },
          { word: "world", start: 0.8, end: 1.2 },
        ],
      },
      599,
      600,
    ),
    [
      { text: "Hello,", start: 599.2, end: 599.7 },
      { text: "world.", start: 599.8, end: 600 },
    ],
  );
  assert.deepEqual(speechWords({ words: [] }, 0, 10), []);
  assert.deepEqual(
    speechWords(
      {
        words: [
          { word: "", start: 0, end: 1 },
          { word: "silence", start: 1, end: 1 },
        ],
      },
      0,
      10,
    ),
    [],
  );
  for (const result of [
    { words: [{ word: "bad", start: -1, end: 1 }] },
    { words: [{ word: "bad", start: 2, end: 1 }] },
    { words: [{ word: "bad", start: 0, end: Infinity }] },
    { text: "No word timings" },
  ]) {
    assert.throws(() => speechWords(result, 0, 10));
  }
});
