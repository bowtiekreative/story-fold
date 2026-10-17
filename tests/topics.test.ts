import test from "node:test";
import assert from "node:assert/strict";
import { groupTopics } from "../apps/server/media.ts";
import { project } from "./fixtures.ts";
import {
  requestTopics,
  topicSettings,
  TopicProviderError,
} from "../apps/server/topics.ts";

const sentences = [{ n: 1, text: "A synthetic interview sentence." }];
const schema = { type: "object", additionalProperties: false, properties: {} };
const output = { topics: [], assignments: [] };

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
