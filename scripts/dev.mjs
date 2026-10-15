import { spawn } from "node:child_process";
const processes = ["dev:server", "dev:web"].map((name) =>
  spawn("npm", ["run", name], {
    stdio: "inherit",
    shell: process.platform === "win32",
  }),
);
let stopping = false;
function stop(code = 0) {
  if (stopping) return;
  stopping = true;
  processes.forEach((p) => p.kill("SIGTERM"));
  process.exitCode = code;
}
processes.forEach((p) => p.on("exit", (code) => stop(code ?? 1)));
process.on("SIGINT", () => stop());
process.on("SIGTERM", () => stop());
