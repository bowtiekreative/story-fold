import { randomUUID } from "node:crypto";
import { promises as fs } from "node:fs";
import path from "node:path";
export const root = path.resolve(process.env.STORYFOLD_ROOT ?? process.cwd());
try {
  process.loadEnvFile(path.join(root, ".env"));
} catch {
  /* optional local configuration */
}
export const dataRoot = path.resolve(
  process.env.STORYFOLD_DATA_DIR ?? path.join(root, "data"),
);
export async function writeJson(file: string, data: unknown) {
  await fs.mkdir(path.dirname(file), { recursive: true });
  const temp = `${file}.${randomUUID()}.tmp`;
  try {
    const handle = await fs.open(temp, "wx", 0o600);
    try {
      await handle.writeFile(JSON.stringify(data, null, 2));
      await handle.sync();
    } finally {
      await handle.close();
    }
    await fs.rename(temp, file);
    if (process.platform !== "win32") {
      const directory = await fs.open(path.dirname(file), "r");
      try {
        await directory.sync();
      } finally {
        await directory.close();
      }
    }
  } finally {
    await fs.rm(temp, { force: true });
  }
}
export async function readJson<T>(file: string): Promise<T> {
  return JSON.parse(await fs.readFile(file, "utf8")) as T;
}
export function projectDir(id: string) {
  if (!/^[a-zA-Z0-9-]{1,60}$/.test(id))
    throw new Error("Invalid project identifier.");
  return path.join(dataRoot, "projects", id);
}
export async function readProject(id: string) {
  return readJson<import("../../packages/timeline/index.ts").Project>(
    path.join(projectDir(id), "project.json"),
  );
}
export async function saveProject(
  project: import("../../packages/timeline/index.ts").Project,
) {
  await writeJson(path.join(projectDir(project.id), "project.json"), project);
}
const projectWrites = new Map<string, Promise<unknown>>();
export async function withProjectWrite<T>(
  id: string,
  work: () => Promise<T>,
): Promise<T> {
  const previous = projectWrites.get(id) ?? Promise.resolve();
  const current = previous.catch(() => {}).then(work);
  projectWrites.set(id, current);
  try {
    return await current;
  } finally {
    if (projectWrites.get(id) === current) projectWrites.delete(id);
  }
}
