import test from "node:test";
import assert from "node:assert/strict";
import {
  sequence,
  frameAt,
  tc25,
  fmt,
  splitSentences,
  buildEdl,
  buildSrt,
  buildFcpxml,
} from "../packages/timeline/index.ts";
import { project, sentence } from "./fixtures.ts";
test("live groups update membership, preserve source order, skip clips and allow intentional repeats", () => {
  const p = project();
  p.sentences.reverse();
  p.sentences.find((s) => s.id === "s2")!.skip = true;
  p.timeline.push({ kind: "clip", sentenceId: "s1" });
  assert.deepEqual(
    sequence(p).clips.map((c) => c.sentence.id),
    ["s1", "s3", "s1"],
  );
  p.sentences.find((s) => s.id === "s1")!.topicId = "two";
  assert.deepEqual(
    sequence(p).spans.map((s) => s.count),
    [0, 2, 1],
  );
});
test("L-cut changes picture one second before the audio without changing total length", () => {
  const p = project();
  p.sentences[0].lcut = 1;
  const sq = sequence(p);
  assert.equal(sq.total, 9);
  assert.equal(sq.clips[0].vEnd, 2);
  assert.equal(sq.clips[1].vStart, 2);
  const f = frameAt(sq, 2.5);
  assert.equal(f.audio?.sentence.id, "s1");
  assert.equal(f.video?.sentence.id, "s2");
  assert.equal(f.videoTime, 7.5);
  assert.equal(f.lcut, true);
  assert.equal(sq.clips.at(-1)!.vEnd, sq.total);
});
test("L-cuts clamp to both neighboring lengths and never apply at the end", () => {
  const p = project();
  p.sentences[0].out = 1.5;
  p.sentences[0].lcut = 2.5;
  p.sentences.at(-1)!.lcut = 2.5;
  const sq = sequence(p);
  assert.equal(sq.clips[0].l, 0.2);
  assert.equal(sq.clips.at(-1)!.l, 0);
  for (let i = 1; i < sq.clips.length; i++)
    assert.equal(sq.clips[i - 1].vEnd, sq.clips[i].vStart);
});
test("many L-cut combinations maintain contiguous positive picture and audio intervals", () => {
  for (let k = 0; k < 80; k++) {
    const p = project();
    p.sentences = Array.from({ length: 20 }, (_, i) => ({
      ...sentence("s" + i, i * 5, 0.3 + ((i + k) % 8) / 2),
      lcut: ((i + k) % 26) / 10,
    }));
    p.timeline = [{ kind: "group", topicId: "one" }];
    const sq = sequence(p);
    assert.equal(sq.clips[0].vStart, 0);
    for (let i = 0; i < sq.clips.length; i++) {
      const c = sq.clips[i];
      assert.ok(c.vEnd > c.vStart);
      if (i) {
        assert.ok(Math.abs(sq.clips[i - 1].aEnd - c.aStart) < 1e-9);
        assert.ok(Math.abs(sq.clips[i - 1].vEnd - c.vStart) < 1e-9);
      }
    }
    assert.equal(sq.clips.at(-1)!.vEnd, sq.total);
  }
});
test("sentence splitter honors punctuation, merges a short neighbor and pads without entering the next speech", () => {
  const words = [
    { text: "Hi.", start: 0.2, end: 0.5 },
    { text: "This", start: 0.6, end: 1 },
    { text: "works!", start: 1.1, end: 1.8 },
    { text: "Next.", start: 2.2, end: 3.4 },
  ];
  const s = splitSentences(words, 4);
  assert.equal(s.length, 2);
  assert.equal(s[0].text, "Hi. This works!");
  assert.ok(s[0].out <= s[1].start);
  assert.equal(s[0].start, 0.2);
  assert.ok(Math.abs(s[0].in - 0.05) < 1e-9);
});
test("long speech splits at the biggest pause, then stays below 20 seconds", () => {
  const words = Array.from({ length: 50 }, (_, i) => ({
    text: i === 49 ? "end." : "word",
    start: i * 0.6,
    end: i * 0.6 + 0.3,
  }));
  const s = splitSentences(words, 31);
  assert.ok(s.length > 1);
  assert.ok(s.every((s) => s.end - s.start <= 20));
  assert.equal(s.flatMap((s) => s.words).length, 50);
});
test("malformed, unordered or out-of-video timestamps are rejected", () => {
  for (const words of [
    [{ text: "No.", start: 2, end: 1 }],
    [{ text: "No.", start: 0, end: NaN }],
    [{ text: "Long.", start: 0, end: 21 }],
    [{ text: "Outside.", start: 10, end: 11 }],
  ])
    assert.throws(() => splitSentences(words, 10));
  assert.throws(() => splitSentences([]));
});
test("a long pause after a split candidate cannot leave a sentence above twenty seconds", () => {
  const words = [
    { text: "First", start: 0, end: 1 },
    { text: "clause,", start: 2, end: 4 },
    { text: "more", start: 5, end: 7 },
    { text: "speech", start: 18, end: 19 },
    { text: "later.", start: 100, end: 102 },
  ];
  const result = splitSentences(words, 103);
  assert.equal(result.flatMap((s) => s.words).length, words.length);
  assert.ok(result.every((s) => s.end - s.start <= 20));
});
test("25fps timecodes carry at frame/minute/hour boundaries and display rounds correctly", () => {
  assert.equal(tc25(59.999), "00:01:00:00");
  assert.equal(tc25(3600.04), "01:00:00:01");
  assert.equal(fmt(59.999), "01:00.0");
});
test("interchange formats use the same picture and audio cuts, escape text and expose crop limits", () => {
  const p = project();
  p.sentences[0].lcut = 1;
  p.sentences[1].cam = "B";
  p.name = "A & B <story>";
  const edl = buildEdl(p);
  assert.ok(edl.includes("00:00:02:00"));
  assert.ok(edl.includes("VIRTUAL CROP B"));
  const srt = buildSrt(p);
  assert.ok(srt.includes("00:00:00,000 --> 00:00:03,000"));
  const xml = buildFcpxml(p);
  assert.ok(xml.includes("A &amp; B &lt;story&gt;"));
  assert.ok(xml.includes('srcEnable="audio"'));
  assert.ok(xml.includes("<spine>"));
});
