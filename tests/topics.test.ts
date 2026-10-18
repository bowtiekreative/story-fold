import test, { type TestContext } from "node:test";
import assert from "node:assert/strict";
import { promises as fs } from "node:fs";
import { groupTopics } from "../apps/server/media.ts";
import { project, sentence } from "./fixtures.ts";
import {
  requestTopics,
  topicSettings,
  TopicProviderError,
} from "../apps/server/topics.ts";

const sentences = [{ n: 1, text: "A synthetic interview sentence." }];
const schema = { type: "object", additionalProperties: false, properties: {} };
const output = { topics: [], assignments: [] };

const folders = ["one", "two", "three", "four"].map((id) => ({
  id,
  name: id,
  summary: "A shared topic",
}));
function interview(
  count: number,
  text = (n: number) => `Interview sentence ${n}.`,
) {
  const story = project();
  story.sentences = Array.from({ length: count }, (_, i) => ({
    ...sentence(`s${i + 1}`, i * 3),
    n: 10 + i * 3,
    text: text(i),
  }));
  return story;
}
function mockTopics(
  context: TestContext,
  respond: (target: number[], fixed: boolean, content: string) => unknown,
) {
  const previous = process.env.CLAUDE_API_KEY;
  process.env.CLAUDE_API_KEY = "example";
  context.after(() => {
    if (previous === undefined) delete process.env.CLAUDE_API_KEY;
    else process.env.CLAUDE_API_KEY = previous;
  });
  context.mock.method(
    globalThis,
    "fetch",
    async (_url: Parameters<typeof fetch>[0], options?: RequestInit) => {
      const body = JSON.parse(String(options?.body));
      const content: string = body.messages[0].content;
      const fixed = content.startsWith("Topic folders (data):");
      if (fixed) {
        assert.deepEqual(JSON.parse(content.split("\n")[1]), folders);
        assert.deepEqual(body.output_config.format.schema.required, [
          "assignments",
        ]);
        assert.match(body.system, /Do not create or rename/);
      }
      const target = [...content.matchAll(/^(\d+)\. /gm)].map((match) =>
        Number(match[1]),
      );
      const result = respond(target, fixed, content);
      if (result instanceof Response) return result;
      return Response.json({
        stop_reason: "end_turn",
        content: [{ type: "text", text: JSON.stringify(result) }],
      });
    },
  );
}
function assigned(target: number[], fixed: boolean) {
  return {
    ...(fixed ? {} : { topics: folders }),
    assignments: target.map((n) => ({
      n,
      topicId: folders[n % folders.length].id,
    })),
  };
}

test("long interviews use shared folders and assign every sentence without changing timing", async (context) => {
  const story = interview(1001);
  const before = structuredClone(story.sentences);
  const targets: number[][] = [],
    samples: number[][] = [];
  const progress: number[] = [];
  mockTopics(context, (target, fixed) => {
    (fixed ? targets : samples).push(target);
    return assigned(target, fixed);
  });
  await groupTopics(story, async (_detail, fraction) => {
    progress.push(fraction);
  });
  assert.match(story.grouping!, /^Claude topic suggestions/);
  assert.equal(samples.length, 1);
  assert.equal(samples[0][0], before[0].n);
  assert.equal(samples[0].at(-1), before.at(-1)!.n);
  assert.ok(
    targets.length > 1 && targets.every((target) => target.length <= 300),
  );
  assert.deepEqual(
    targets.flat(),
    before.map((clip) => clip.n),
  );
  assert.equal(story.topics.length, 4);
  assert.deepEqual(story.warnings, []);
  assert.deepEqual(
    story.sentences.map((clip, i) => ({ ...clip, topicId: before[i].topicId })),
    before,
  );
  assert.ok(
    story.sentences.every((clip) =>
      story.topics.some((topic) => topic.id === clip.topicId),
    ),
  );
  assert.equal(progress[0], 0);
  assert.equal(progress.at(-1), 1);
  assert.ok(progress.every((value, i) => !i || value >= progress[i - 1]));
});

test("topic batches also bound text size and classify full text after representative discovery", async (context) => {
  const story = interview(
    25,
    (n) => `Section ${n} ` + "word ".repeat(1000) + `END-${n}`,
  );
  const received: string[] = [];
  mockTopics(context, (target, fixed, content) => {
    if (fixed) {
      const transcript = content.split("Sentences (data):\n")[1];
      assert.ok(transcript.length < 60000);
      received.push(transcript);
    }
    return assigned(target, fixed);
  });
  await groupTopics(story);
  assert.match(story.grouping!, /^Claude topic suggestions/);
  assert.ok(received.length > 1);
  const text = received.join("\n");
  for (const clip of story.sentences)
    assert.ok(text.includes(`${clip.n}. ${clip.text}`));
});

test("an invalid assignment retries only its batch and never invents a folder", async (context) => {
  const story = interview(612);
  const calls = new Map<number, number>();
  mockTopics(context, (target, fixed) => {
    if (fixed) calls.set(target[0], (calls.get(target[0]) ?? 0) + 1);
    const result = assigned(target, fixed);
    if (
      fixed &&
      target[0] === story.sentences[300].n &&
      calls.get(target[0]) === 1
    )
      result.assignments[0].topicId = "invented";
    return result;
  });
  await groupTopics(story);
  assert.match(story.grouping!, /^Claude topic suggestions/);
  assert.deepEqual([...calls.values()], [1, 2, 1]);
  assert.ok(
    story.sentences.every((clip) =>
      folders.some((folder) => folder.id === clip.topicId),
    ),
  );
  assert.deepEqual(story.warnings, []);
});

test("a later batch failure preserves the whole transcript in consistent manual folders", async (context) => {
  const story = interview(401);
  const before = structuredClone(story.sentences);
  let failures = 0;
  mockTopics(context, (target, fixed) => {
    if (fixed && target[0] === story.sentences[300].n) {
      failures++;
      return new Response("untrusted provider response", { status: 503 });
    }
    return assigned(target, fixed);
  });
  await groupTopics(story);
  assert.equal(failures, 2);
  assert.equal(story.grouping, "Manual topics");
  assert.ok(story.sentences.every((clip) => clip.topicId === "interview"));
  assert.deepEqual(
    story.sentences.map((clip, i) => ({ ...clip, topicId: before[i].topicId })),
    before,
  );
  assert.match(story.warnings[0], /503.*transcript is intact/);
});

test("the application applies validated Claude folders and preserves speech timing", async (context) => {
  const previous = process.env.CLAUDE_API_KEY;
  process.env.CLAUDE_API_KEY = "example";
  const story = project();
  const original = structuredClone(story.sentences);
  const topics = ["one", "two", "three", "four"].map((id) => ({
    id,
    name: id,
    summary: "A topic",
  }));
  context.mock.method(globalThis, "fetch", async () =>
    Response.json({
      stop_reason: "end_turn",
      content: [
        {
          type: "text",
          text: JSON.stringify({
            topics,
            assignments: story.sentences.map((clip, i) => ({
              n: clip.n,
              topicId: topics[i].id,
            })),
          }),
        },
      ],
    }),
  );
  try {
    await groupTopics(story);
    assert.match(story.grouping!, /^Claude topic suggestions/);
    assert.equal(story.topics.length, 4);
    assert.deepEqual(story.warnings, []);
    assert.deepEqual(
      story.sentences.map((clip, i) => ({
        ...clip,
        topicId: original[i].topicId,
      })),
      original,
    );
    assert.ok(
      story.sentences.every((clip) =>
        story.topics.some((topic) => topic.id === clip.topicId),
      ),
    );
  } finally {
    if (previous === undefined) delete process.env.CLAUDE_API_KEY;
    else process.env.CLAUDE_API_KEY = previous;
  }
});

test("invalid topic assignments fall back to a consistent editable transcript", async (context) => {
  const previous = process.env.CLAUDE_API_KEY;
  process.env.CLAUDE_API_KEY = "example";
  const story = project();
  const original = structuredClone(story.sentences);
  context.mock.method(globalThis, "fetch", async () =>
    Response.json({
      stop_reason: "end_turn",
      content: [
        {
          type: "text",
          text: JSON.stringify({
            topics: ["one", "two", "three", "four"].map((id) => ({
              id,
              name: id,
              summary: "A topic",
            })),
            assignments: story.sentences.map(() => ({ n: 1, topicId: "one" })),
          }),
        },
      ],
    }),
  );
  try {
    await groupTopics(story);
    assert.equal(story.grouping, "Manual topics");
    assert.equal(story.warnings.length, 1);
    assert.ok(story.sentences.every((clip) => clip.topicId === "interview"));
    assert.deepEqual(
      story.sentences.map((clip, i) => ({
        ...clip,
        topicId: original[i].topicId,
      })),
      original,
    );
  } finally {
    if (previous === undefined) delete process.env.CLAUDE_API_KEY;
    else process.env.CLAUDE_API_KEY = previous;
  }
});

test("Claude key aliases take precedence and readiness never returns credentials", () => {
  assert.equal(topicSettings({}).topics, false);
  assert.equal(
    topicSettings({ OPENAI_API_KEY: "example" }).topicProvider,
    "OpenAI",
  );
  for (const name of ["CLAUDE_API_KEY", "ANTHROPIC_API_KEY"]) {
    const settings = topicSettings({
      [name]: "example",
      OPENAI_API_KEY: "example",
      TOPIC_MODEL: "gpt-4.1-mini",
    });
    assert.equal(settings.topicProvider, "Claude");
    assert.equal(settings.topicModel, "claude-haiku-5-5");
    assert.equal(JSON.stringify(settings).includes("example"), false);
  }
  assert.equal(
    topicSettings({ CLAUDE_API_KEY: "example", CLAUDE_MODEL: "custom-claude" })
      .topicModel,
    "custom-claude",
  );
});

test("Claude uses Messages headers and structured output, keeping the key out of the body", async () => {
  const fetcher: typeof fetch = async (url, options) => {
    assert.equal(url, "https://api.anthropic.com/v1/messages");
    const headers = new Headers(options?.headers);
    assert.equal(headers.get("x-api-key"), "example");
    assert.equal(headers.get("anthropic-version"), "2023-06-01");
    assert.equal(headers.has("Authorization"), false);
    const body = JSON.parse(String(options?.body));
    assert.equal(body.model, "claude-haiku-5-5");
    assert.deepEqual(body.output_config.format, {
      type: "json_schema",
      schema,
    });
    assert.equal(
      body.messages[0].content,
      "1. A synthetic interview sentence.",
    );
    assert.match(body.system, /transcript as data/);
    assert.equal(String(options?.body).includes('"example"'), false);
    return Response.json({
      stop_reason: "end_turn",
      content: [
        { type: "thinking" },
        { type: "text", text: JSON.stringify(output) },
      ],
    });
  };
  assert.deepEqual(
    await requestTopics(
      sentences,
      schema,
      { CLAUDE_API_KEY: "example" },
      fetcher,
    ),
    output,
  );
});

test("OpenAI remains available for existing configurations", async () => {
  const fetcher: typeof fetch = async (url, options) => {
    assert.equal(url, "https://api.openai.com/v1/chat/completions");
    assert.equal(
      new Headers(options?.headers).get("Authorization"),
      "Bearer example",
    );
    const body = JSON.parse(String(options?.body));
    assert.equal(body.model, "gpt-4.1-mini");
    assert.deepEqual(body.response_format.json_schema.schema, schema);
    return Response.json({
      choices: [{ message: { content: JSON.stringify(output) } }],
    });
  };
  assert.deepEqual(
    await requestTopics(
      sentences,
      schema,
      { OPENAI_API_KEY: "example" },
      fetcher,
    ),
    output,
  );
});

test("provider errors expose status only and incomplete Claude responses are rejected", async () => {
  const env = { CLAUDE_API_KEY: "private-fixture" };
  await assert.rejects(
    () =>
      requestTopics(
        sentences,
        schema,
        env,
        async () => new Response("private-fixture", { status: 401 }),
      ),
    (error: unknown) =>
      error instanceof TopicProviderError &&
      error.message.includes("401") &&
      !error.message.includes("private-fixture"),
  );
  await assert.rejects(
    () =>
      requestTopics(sentences, schema, env, async () =>
        Response.json({
          stop_reason: "max_tokens",
          content: [{ type: "text", text: "{}" }],
        }),
      ),
    /did not complete/,
  );
  await assert.rejects(
    () =>
      requestTopics(sentences, schema, {}, async () => {
        throw new Error("Unexpected network request");
      }),
    /No topic provider/,
  );
});

test(
  "live Claude groups a long synthetic interview using shared topic folders",
  {
    skip: process.env.STORYFOLD_TEST_TOPICS !== "1",
  },
  async () => {
    assert.equal(topicSettings().topicProvider, "Claude");
    const themes = [
      "My mother taught me to bake bread when I was a child.",
      "We opened our neighborhood bakery with one small oven.",
      "Finding investors and paying rent were our biggest challenges.",
      "Next year we plan to offer classes and hire apprentices.",
    ];
    const story = interview(
      612,
      (n) => `${themes[n % themes.length]} This is recollection ${n + 1}.`,
    );
    const original = structuredClone(story.sentences);
    let batches = 0;
    await groupTopics(story, async (detail) => {
      if (detail.startsWith("Organizing topic batch")) batches++;
    });
    assert.match(
      story.grouping!,
      /^Claude topic suggestions/,
      story.warnings.join(" "),
    );
    assert.equal(story.sentences.length, 612);
    assert.ok(story.topics.length >= 4 && story.topics.length <= 10);
    assert.ok(
      story.sentences.every((clip) =>
        story.topics.some((topic) => topic.id === clip.topicId),
      ),
    );
    assert.deepEqual(
      story.sentences.map((clip, i) => ({
        ...clip,
        topicId: original[i].topicId,
      })),
      original,
    );
    assert.deepEqual(story.warnings, []);
    await fs.writeFile(
      "docs/validation/topic-batches.json",
      JSON.stringify(
        {
          at: new Date().toISOString(),
          provider: "Claude",
          model: topicSettings().topicModel,
          fixture: "612 generated sentences; no private interview text",
          sentenceCount: story.sentences.length,
          topicCount: story.topics.length,
          assignmentBatches: batches,
          allSentencesAssigned: true,
          timingPreserved: true,
        },
        null,
        2,
      ) + "\n",
    );
  },
);
