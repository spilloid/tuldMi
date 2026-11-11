#!/usr/bin/env node
// Fetches sherpa-onnx's prebuilt static libraries for this host into .sherpa-onnx/lib, which
// .cargo/config.toml hands to sherpa-onnx-sys as SHERPA_ONNX_LIB_DIR. The archive is pinned by
// SHA-256 (from the GitHub release's asset digests); a mismatch fails and nothing is extracted.
//
// These are the *no-TTS* builds: the full builds statically include espeak-ng (GPL-3.0), which
// would otherwise end up inside tuldmi-speakers even though nothing calls it. sherpa-onnx-sys
// still names espeak-ng, piper_phonemize and ucd on the link line unconditionally, so empty
// archives stand in for them (nothing references their symbols; the link would fail if
// anything did). Windows archives use the static C runtime (/MT), matching +crt-static.
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
  "win32-x64": ["win-x64-static-MT-Release-no-tts-lib", "f0aa074ddc39553b30208b53e84c12275d09702911c9faa8d34fa932428e4641"],
  "darwin-arm64": ["osx-arm64-static-no-tts-lib", "3d7f9b8a496694af13d9802c33b8133231e397bdef302f543d19468765e83136"],
  "linux-x64": ["linux-x64-static-no-tts-lib", "36f2ebd0b9aa09248ff6461a30dcb7edfe7eecf2deb67dfb0ceb3aaf4906051f"],
};
/** Libraries sherpa-onnx-sys always links that only the TTS build has. */
const TTS_ONLY = ["espeak-ng", "piper_phonemize", "ucd"];

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
for (const name of TTS_ONLY) {
  const file = join(out, "lib", process.platform === "win32" ? `${name}.lib` : `lib${name}.a`);
  if (existsSync(file)) throw new Error(`${file} exists: this is not a no-TTS build`);
  writeFileSync(file, "!<arch>\n"); // an empty ar archive
}
writeFileSync(marker, `${stem} ${sha256}\n`);
console.log(`sherpa-onnx libraries: ${join(out, "lib")}`);
