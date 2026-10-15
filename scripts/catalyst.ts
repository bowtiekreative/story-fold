import { promises as fs } from "node:fs";
import path from "node:path";
const [command, ...args] = process.argv.slice(2);
const base = process.env.STORYFOLD_URL ?? "http://127.0.0.1:4310";
async function request(route: string, body?: unknown) {
  const token = (await fetch(base + "/api/session").then((r) => r.json())) as {
    token: string;
  };
  const response = await fetch(
    base + "/api" + route,
    body
      ? {
          method: "POST",
          headers: {
            "Content-Type": "application/json",
            "X-Storyfold-Token": token.token,
          },
          body: JSON.stringify(body),
        }
      : {},
  );
  const data = await response.json();
  if (!response.ok) throw new Error(data.error);
  return data;
}
if (command === "status") {
  const data = await request("/catalyst");
  console.log(
    JSON.stringify(
      { pipeline: data.pipeline, paused: data.control.paused, run: data.run },
      null,
      2,
    ),
  );
} else if (command === "progress") {
  const message = args.join(" ").trim();
  if (!message)
    throw new Error("Usage: npm run catalyst -- progress <message>");
  const file = path.resolve(".catalyst/pipeline.json"),
    pipeline = JSON.parse(await fs.readFile(file, "utf8"));
  pipeline.message = message;
  pipeline.updatedAt = new Date().toISOString();
  const temp = file + ".tmp";
  await fs.writeFile(temp, JSON.stringify(pipeline, null, 2));
  await fs.rename(temp, file);
  console.log("Progress saved. Catalyst refreshes automatically.");
} else if (command === "checks")
  console.log(
    JSON.stringify(
      await request("/catalyst/checks", {
        kind: args[0] === "build" ? "build" : "test",
      }),
      null,
      2,
    ),
  );
else if (command === "pause" || command === "resume")
  console.log(
    await request("/catalyst/control", { paused: command === "pause" }),
  );
else
  console.log(
    "Catalyst Solo\n  npm run catalyst -- status\n  npm run catalyst -- progress <message>\n  npm run catalyst -- checks build|test\n  npm run catalyst -- pause|resume\n\nDashboard: " +
      base +
      "/catalyst",
  );
