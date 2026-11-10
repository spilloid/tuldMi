#!/usr/bin/env node
// Fetches sherpa-onnx's prebuilt static libraries for this host into .sherpa-onnx/lib, which
// .cargo/config.toml hands to sherpa-onnx-sys as SHERPA_ONNX_LIB_DIR. The archive is pinned by
// SHA-256 (from the GitHub release's asset digests); a mismatch fails and nothing is extracted.
// Idempotent: skips the download when .sherpa-onnx/lib was extracted from the same archive.
//   node scripts/sherpa-libs.mjs
import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// Must match the sherpa-onnx crate version in Cargo.lock.
const VERSION = "1.13.8";
const ARCHIVES = {
  "win32-x64": ["win-x64-static-MT-Release-lib", "56ffcf3c454c1f14f7bc9887286cc8143e7e542dc632804e1c447d5f8d534eaf"],
  "darwin-arm64": ["osx-arm64-static-lib", "9091bf160dc7fdacedbc906b212badf53c2993f4e5277a0e03998e96c31d60da"],
  "linux-x64": ["linux-x64-static-lib", "e1fdc5b67530e15741ef897fa5ffff297056f3bf0c6d829a27af9225a4c4b5a6"],
};

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, ".sherpa-onnx");
const platform = `${process.platform}-${process.arch}`;
const pinned = ARCHIVES[platform];
if (!pinned) throw new Error(`no pinned sherpa-onnx libraries for ${platform}`);
const [flavor, sha256] = pinned;
const stem = `sherpa-onnx-v${VERSION}-${flavor}`;
const marker = join(out, "lib", ".archive");

if (existsSync(marker) && readFileSync(marker, "utf8").trim() === `${stem} ${sha256}`) {
  console.log(`sherpa-onnx libraries present: ${join(out, "lib")}`);
  process.exit(0);
}

const url = `https://github.com/k2-fsa/sherpa-onnx/releases/download/v${VERSION}/${stem}.tar.bz2`;
console.log(`fetching ${url}`);
const response = await fetch(url);
if (!response.ok) throw new Error(`download failed: HTTP ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
const actual = createHash("sha256").update(bytes).digest("hex");
if (actual !== sha256) throw new Error(`sha256 mismatch for ${stem}: got ${actual}, pinned ${sha256}`);

rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });
const archive = join(out, `${stem}.tar.bz2`);
writeFileSync(archive, bytes);
// bsdtar (Windows 10+, macOS) and GNU tar both read bzip2 with -j.
execFileSync("tar", ["-xjf", `${stem}.tar.bz2`], { cwd: out, stdio: "inherit" });
rmSync(archive);
renameSync(join(out, stem, "lib"), join(out, "lib"));
rmSync(join(out, stem), { recursive: true, force: true });
writeFileSync(marker, `${stem} ${sha256}\n`);
console.log(`sherpa-onnx libraries: ${join(out, "lib")}`);
