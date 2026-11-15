#!/usr/bin/env node
// Builds the sidecars that ship beside the app (release) and puts them where Tauri's externalBin
// expects them: src-tauri/binaries/<name>-<target triple>[.exe].
//   - tuldmi-mcp       the MCP server (docs/design/mcp.md)
//   - tuldmi-speakers  the speaker-separation worker (docs/design/speakers.md); first fetches
//                      sherpa-onnx's pinned, hash-checked libraries (scripts/sherpa-libs.mjs)
// Run before `tauri build --config src-tauri/tauri.bundle.conf.json` (scripts/release.ps1 and
// the macOS workflow do).
//   node scripts/build-sidecars.mjs [target-triple]   (default: the host)
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
const target = process.argv[2] ?? host;
if (!target) throw new Error("could not determine the target triple");

execFileSync(process.execPath, [join(root, "scripts", "sherpa-libs.mjs")], { cwd: root, stdio: "inherit" });

const sidecars = ["tuldmi-mcp", "tuldmi-speakers"];
const args = ["build", "--release", "--locked", ...sidecars.flatMap((p) => ["-p", p])];
if (process.argv[2]) args.push("--target", target);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

// Cargo owns target-directory resolution (environment, config, and CLI defaults).
// The Windows GPU release uses a short external CARGO_TARGET_DIR for native shaders.
const metadata = JSON.parse(execFileSync("cargo", ["metadata", "--no-deps", "--format-version", "1", "--locked"], {
  cwd: root, encoding: "utf8",
}));
const ext = target.includes("windows") ? ".exe" : "";
for (const name of sidecars) {
  const built = join(metadata.target_directory, ...(process.argv[2] ? [target] : []), "release", `${name}${ext}`);
  const dest = join(root, "src-tauri", "binaries", `${name}-${target}${ext}`);
  mkdirSync(dirname(dest), { recursive: true });
  copyFileSync(built, dest);
  console.log(`${name} -> ${dest}`);
}
