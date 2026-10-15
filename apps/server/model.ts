import { z } from "zod";
import type { Project } from "../../packages/timeline/index.ts";
const finite = z.number().finite();
export const wordSchema = z
  .object({
    text: z.string().trim().min(1).max(200),
    start: finite.min(0),
    end: finite.min(0),
  })
  .refine((w) => w.end > w.start);
export const topicSchema = z.object({
  id: z.string().regex(/^[\w-]{1,80}$/),
  name: z.string().trim().min(1).max(80),
  accent: z.enum(["violet", "coral", "teal", "amber", "sky", "lime"]),
  summary: z.string().max(500).optional(),
});
export const timelineSchema = z
  .array(
    z.discriminatedUnion("kind", [
      z.object({ kind: z.literal("group"), topicId: z.string() }),
      z.object({ kind: z.literal("clip"), sentenceId: z.string() }),
    ]),
  )
  .max(2000);
export const settingsSchema = z.object({
  ratio: z.enum(["16:9", "9:16", "1:1"]),
  captions: z.boolean(),
  chapters: z.boolean(),
});
export const editSchema = z.object({
  revision: z.number().int().min(0),
  name: z.string().trim().min(1).max(120),
  sentences: z
    .array(
      z.object({
        id: z.string(),
        text: z.string().trim().min(1).max(4000),
        topicId: z.string(),
        cam: z.enum(["A", "B", "C"]),
        in: finite.min(0),
        out: finite.min(0),
        lcut: finite.min(0).max(2.5),
        skip: z.boolean(),
      }),
    )
    .max(20000),
  topics: z.array(topicSchema).min(1).max(100),
  timeline: timelineSchema,
  exportSettings: settingsSchema,
  offsets: z
    .record(z.enum(["A", "B", "C"]), finite.min(-7200).max(7200))
    .optional(),
});
export function applyProjectEdit(project: Project, input: unknown): Project {
  const edit = editSchema.parse(input);
  if (edit.revision !== project.revision)
    throw Object.assign(
      new Error(
        "This project changed in another window. Reload before saving.",
      ),
      { status: 409 },
    );
  if (
    edit.sentences.length !== project.sentences.length ||
    new Set(edit.sentences.map((s) => s.id)).size !== project.sentences.length
  )
    throw new Error("Every original sentence must be present exactly once.");
  const topicIds = new Set(edit.topics.map((t) => t.id));
  if (topicIds.size !== edit.topics.length)
    throw new Error("Topic identifiers must be unique.");
  const sentences = edit.sentences.map((s) => {
    const original = project.sentences.find((o) => o.id === s.id);
    if (
      !original ||
      !topicIds.has(s.topicId) ||
      !project.cameras.some((c) => c.id === s.cam)
    )
      throw new Error("Unknown sentence, topic or camera.");
    const min = Math.max(0, original.start - 1.5),
      max = Math.min(project.cameras[0].duration, original.end + 1.5);
    if (s.in < min - 0.001 || s.out > max + 0.001 || s.out - s.in < 0.3 - 0.001)
      throw new Error(
        "A cut must stay within 1.5 seconds of its sentence and be at least 0.3 seconds long.",
      );
    return { ...original, ...s };
  });
  if (
    edit.timeline.some((i) =>
      i.kind === "group"
        ? !topicIds.has(i.topicId)
        : !sentences.some((s) => s.id === i.sentenceId),
    )
  )
    throw new Error("Timeline refers to an unknown sentence or topic.");
  const cameras = project.cameras.map((c) => ({
    ...c,
    offset: c.id === "A" || c.virtual ? 0 : (edit.offsets?.[c.id] ?? c.offset),
  }));
  return {
    ...project,
    name: edit.name,
    sentences,
    topics: edit.topics,
    timeline: edit.timeline,
    exportSettings: edit.exportSettings,
    cameras,
    revision: project.revision + 1,
  };
}
