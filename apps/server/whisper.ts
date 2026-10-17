import { promises as fs } from "node:fs";
import path from "node:path";
import { dataRoot, root } from "./storage.ts";
import { z } from "zod";
import { wordSchema } from "./model.ts";

export function whisperPaths(base = dataRoot) {
  const runtime = path.join(base, "runtime", "whisper");
  return {
    python: path.join(
      runtime,
      process.platform === "win32" ? "Scripts/python.exe" : "bin/python",
    ),
    model: path.join(base, "models", "whisper-small"),
    ready: path.join(runtime, "ready.json"),
    worker: path.join(root, "scripts", "whisper.py"),
  };
}

export async function speechStatus(
  base = dataRoot,
  env: NodeJS.ProcessEnv = process.env,
) {
  const paths = whisperPaths(base);
  const needed = [
    paths.python,
    paths.ready,
    path.join(paths.model, "model.bin"),
    path.join(paths.model, "config.json"),
    path.join(paths.model, "tokenizer.json"),
  ];
  const local = (
    await Promise.all(
      needed.map((file) =>
        fs.access(file).then(
          () => true,
          () => false,
        ),
      ),
    )
  ).every(Boolean);
  const remote = Boolean(env.OPENAI_API_KEY?.trim());
  return {
    transcription: local || remote,
    speechProvider: local
      ? "Local Whisper"
      : remote
        ? "OpenAI Whisper"
        : "Setup needed",
  };
}

const chunkSchema = z.object({
  words: z
    .array(
      z
        .object({
          word: z.string().max(200),
          start: z.number().finite().min(0),
          end: z.number().finite().min(0),
        })
        .refine((word) => word.end >= word.start),
    )
    .max(20000),
  text: z.string().optional(),
});

export function speechWords(input: unknown, offset: number, duration: number) {
  const result = chunkSchema.parse(input);
  const tokens = (result.text ?? "").split(/\s+/).filter(Boolean);
  const normalize = (text: string) =>
    text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
  let index = 0;
  const words = [];
  for (const word of result.words) {
    let text = word.word.trim();
    if (tokens[index] && normalize(tokens[index]) === normalize(text))
      text = tokens[index++];
    const start = word.start + offset;
    const end = Math.min(word.end + offset, duration);
    if (text && end > start) words.push(wordSchema.parse({ text, start, end }));
  }
  return words;
}
