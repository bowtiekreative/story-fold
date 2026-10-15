import type { Project, Sentence } from "../packages/timeline/index.ts";
export const sentence = (
  id: string,
  start: number,
  dur = 3,
  topicId = "one",
): Sentence => ({
  id,
  n: Number(id.replace(/\D/g, "")) || 1,
  text: `Sentence ${id}.`,
  words: [],
  start,
  end: start + dur,
  in: start,
  out: start + dur,
  topicId,
  cam: "A",
  lcut: 0,
  skip: false,
});
export function project(): Project {
  return {
    id: "fixture",
    name: "Test story",
    revision: 0,
    createdAt: "2026-10-15T00:00:00Z",
    status: "ready",
    cameras: [
      {
        id: "A",
        name: "Wide",
        sourcePath: "source-A.mp4",
        proxyPath: "proxy-A.mp4",
        duration: 40,
        offset: 0,
      },
      {
        id: "B",
        name: "Close-up",
        sourcePath: "source-A.mp4",
        proxyPath: "proxy-A.mp4",
        duration: 40,
        offset: 0,
        virtual: { scale: 1.65, x: 0.5, y: 0.5 },
      },
    ],
    sentences: [
      sentence("s1", 1),
      sentence("s2", 7),
      sentence("s3", 12, 3, "two"),
    ],
    topics: [
      { id: "one", name: "First topic", accent: "violet" },
      { id: "two", name: "Second topic", accent: "teal" },
    ],
    timeline: [
      { kind: "group", topicId: "one" },
      { kind: "group", topicId: "two" },
    ],
    exportSettings: { ratio: "16:9", captions: true, chapters: true },
    progress: [],
    warnings: [],
  };
}
