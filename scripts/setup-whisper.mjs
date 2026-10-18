import { spawn } from "node:child_process";
import { access, mkdir, writeFile, rename, rm } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
try {
  process.loadEnvFile(path.join(root, ".env"));
} catch {
  /* optional */
}
const data = path.resolve(
  process.env.STORYFOLD_DATA_DIR || path.join(root, "data"),
);
const runtime = path.join(data, "runtime", "whisper");
const python = path.join(
  runtime,
  process.platform === "win32" ? "Scripts/python.exe" : "bin/python",
);
const model = path.join(data, "models", "whisper-small");
const ready = path.join(runtime, "ready.json");

function run(bin, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(bin, args, {
      cwd: root,
      stdio: "inherit",
      windowsHide: true,
    });
    child.once("error", () =>
      reject(
        new Error(
          "Could not start " +
            path.basename(bin) +
            ". Install Python 3.9 or later.",
        ),
      ),
    );
    child.once("close", (code) =>
      code === 0
        ? resolve()
        : reject(
            new Error(
              "Whisper setup did not finish. Fix the error above and rerun setup.",
            ),
          ),
    );
  });
}

try {
  console.log(
    "Setting up local Whisper. The model download is about 500 MB; footage is not uploaded.",
  );
  await mkdir(runtime, { recursive: true });
  await rm(ready, { force: true });
  const exists = await access(python).then(
    () => true,
    () => false,
  );
  if (!exists)
    await run(process.platform === "win32" ? "python" : "python3", [
      "-m",
      "venv",
      runtime,
    ]);
  await run(python, [
    "-m",
    "pip",
    "--isolated",
    "install",
    "--index-url",
    "https://pypi.org/simple",
    "--only-binary=:all:",
    "faster-whisper==1.2.1",
    // faster-whisper 1.2.1 passes an audio-decoder option removed in PyAV 19.
    "av>=11,<19",
    "huggingface-hub<1",
  ]);
  console.log(
    "Downloading and checking the small Whisper model from the official Systran repository on Hugging Face...",
  );
  await run(python, [
    path.join(root, "scripts", "whisper.py"),
    "--prepare",
    "--model",
    model,
  ]);
  const temp = ready + "." + randomUUID() + ".tmp";
  try {
    await writeFile(
      temp,
      JSON.stringify({
        version: "1.2.1",
        model: "small",
        at: new Date().toISOString(),
      }),
      { mode: 0o600 },
    );
    await rename(temp, ready);
  } finally {
    await rm(temp, { force: true });
  }
  console.log(
    "Local Whisper is ready. Refresh Storyfold and upload a video; no transcript JSON is needed.",
  );
} catch (error) {
  console.error(error.message);
  process.exitCode = 1;
}
