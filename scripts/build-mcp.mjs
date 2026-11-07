#!/usr/bin/env node
// Builds tuldmi-mcp (release) and puts it where Tauri's externalBin expects it:
// src-tauri/binaries/tuldmi-mcp-<target triple>[.exe]. Run before `tauri build --config
// src-tauri/tauri.bundle.conf.json` (scripts/release.ps1 and the macOS workflow do).
//   node scripts/build-mcp.mjs [target-triple]   (default: the host)
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (\S+)$/m)?.[1];
const target = process.argv[2] ?? host;
if (!target) throw new Error("could not determine the target triple");

const args = ["build", "--release", "--locked", "-p", "tuldmi-mcp"];
if (process.argv[2]) args.push("--target", target);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const ext = target.includes("windows") ? ".exe" : "";
const built = join(root, "target", ...(process.argv[2] ? [target] : []), "release", `tuldmi-mcp${ext}`);
const dest = join(root, "src-tauri", "binaries", `tuldmi-mcp-${target}${ext}`);
mkdirSync(dirname(dest), { recursive: true });
copyFileSync(built, dest);
console.log(`tuldmi-mcp -> ${dest}`);
