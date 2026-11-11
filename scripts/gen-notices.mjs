#!/usr/bin/env node
// Generates THIRD_PARTY_NOTICES.md from the Rust (cargo metadata) and npm
// (installed node_modules) dependency graphs. No dependencies beyond Node.
//
// Usage: node scripts/gen-notices.mjs
// Prints flagged licenses (missing, or GPL/LGPL/AGPL/MPL/EPL/CDDL/SSPL/BUSL/
// Commercial) to stdout as "name@version: expression".

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
// Everything that ships: the app and both sidecars installed beside it.
const ROOT_CRATES = ['tuldmi', 'tuldmi-engine', 'tuldmi-mcp', 'tuldmi-speakers'];
const FLAG_RE = /GPL|MPL|EPL|CDDL|SSPL|BUSL|commercial/i;
const MISSING = '(missing)';

const readJson = (p) => JSON.parse(readFileSync(p, 'utf8').replace(/^﻿/, ''));

function normRepo(url) {
  if (!url) return '';
  let s = String(url).trim().replace(/^git\+/, '').replace(/\.git$/, '');
  s = s
    .replace(/^git:\/\//, 'https://')
    .replace(/^ssh:\/\/git@/, 'https://')
    .replace(/^git@([^:]+):/, 'https://$1/')
    .replace(/^github:/, 'https://github.com/');
  if (/^[\w.-]+\/[\w.-]+$/.test(s)) s = `https://github.com/${s}`;
  return s;
}

function isMissing(lic) {
  return !lic || /^(unknown|none)$/i.test(lic.trim());
}

// ---------- Rust ----------
function rustPackages() {
  const raw = execFileSync('cargo', ['metadata', '--format-version', '1', '--locked'], {
    cwd: ROOT,
    encoding: 'utf8',
    maxBuffer: 512 * 1024 * 1024,
  });
  const meta = JSON.parse(raw);
  const pkgById = new Map(meta.packages.map((p) => [p.id, p]));
  const nodeById = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const workspace = new Set(meta.workspace_members);

  const roots = meta.packages
    .filter((p) => ROOT_CRATES.includes(p.name) && workspace.has(p.id))
    .map((p) => p.id);
  if (roots.length !== ROOT_CRATES.length) {
    throw new Error(`expected workspace packages ${ROOT_CRATES.join(', ')}, found ${roots.length}`);
  }

  // Walk the resolved graph from the roots. An edge counts only if it is
  // a normal or build edge somewhere (dev-only edges are not shipped).
  const seen = new Set(roots);
  const queue = [...roots];
  while (queue.length) {
    const node = nodeById.get(queue.shift());
    for (const dep of node?.deps ?? []) {
      const shipped = !dep.dep_kinds?.length || dep.dep_kinds.some((k) => k.kind !== 'dev');
      if (!shipped || seen.has(dep.pkg)) continue;
      seen.add(dep.pkg);
      queue.push(dep.pkg);
    }
  }

  const out = new Map();
  for (const id of seen) {
    if (workspace.has(id)) continue;
    const p = pkgById.get(id);
    const key = `${p.name}@${p.version}`;
    if (out.has(key)) continue;
    out.set(key, {
      name: p.name,
      version: p.version,
      license: (p.license ?? '').trim(),
      repository: normRepo(p.repository),
      dir: dirname(p.manifest_path),
      authors: (p.authors ?? []).map((a) => a.replace(/\s*<[^>]*>/, '').trim()).filter(Boolean),
    });
  }
  return [...out.values()];
}

// ---------- npm ----------
function resolvePkgDir(name, fromDir) {
  let dir = fromDir;
  for (;;) {
    const cand = join(dir, 'node_modules', name);
    if (existsSync(join(cand, 'package.json'))) return cand;
    const parent = dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

function npmLicense(pj) {
  if (typeof pj.license === 'string') return pj.license.trim();
  if (pj.license && typeof pj.license === 'object') return (pj.license.type ?? '').trim();
  if (Array.isArray(pj.licenses)) {
    return pj.licenses
      .map((l) => (typeof l === 'string' ? l : l?.type))
      .filter(Boolean)
      .join(' OR ');
  }
  if (pj.licenses && typeof pj.licenses === 'object') return (pj.licenses.type ?? '').trim();
  return '';
}

function npmPackages() {
  const rootPj = readJson(join(ROOT, 'package.json'));
  const visited = new Set();
  const out = new Map();
  const queue = [];
  const notInstalled = [];

  const enqueue = (fromDir, pj) => {
    // production edges only: dependencies + optionalDependencies (skip devDependencies)
    for (const field of ['dependencies', 'optionalDependencies']) {
      for (const name of Object.keys(pj[field] ?? {})) {
        const dir = resolvePkgDir(name, fromDir);
        if (dir) queue.push(dir);
        else if (field === 'dependencies') notInstalled.push(name);
      }
    }
  };

  enqueue(ROOT, rootPj);
  while (queue.length) {
    const dir = queue.shift();
    if (visited.has(dir)) continue;
    visited.add(dir);
    const pj = readJson(join(dir, 'package.json'));
    const key = `${pj.name}@${pj.version}`;
    if (!out.has(key)) {
      out.set(key, {
        name: pj.name,
        version: pj.version,
        license: npmLicense(pj),
        repository: normRepo(typeof pj.repository === 'string' ? pj.repository : pj.repository?.url),
        dir,
        authors: [typeof pj.author === 'string' ? pj.author : pj.author?.name].filter(Boolean).map((a) => a.replace(/\s*[<(].*$/, '').trim()),
      });
    }
    enqueue(dir, pj);
  }
  if (notInstalled.length) {
    console.error(`warning: production dependencies not installed: ${notInstalled.join(', ')}`);
  }
  return [...out.values()];
}

// ---------- license texts ----------
// Every license, copyright and notice file each shipped package publishes, so the installed app
// carries them (MIT, BSD and Apache-2.0 all require a copy). Identical texts are written once,
// under every package that uses them.
const LICENSE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense|authors)([-_.].*)?$/i;

function licenseTexts(rows) {
  const byText = new Map();
  const none = [];
  for (const r of rows) {
    let files = [];
    try {
      files = readdirSync(r.dir, { withFileTypes: true })
        .filter((e) => e.isFile() && LICENSE_FILE.test(e.name) && !/\.(rs|js|ts|json|html)$/i.test(e.name))
        .map((e) => e.name)
        .sort();
    } catch {}
    if (!files.length) {
      none.push(r);
      continue;
    }
    for (const f of files) {
      const text = readFileSync(join(r.dir, f), 'utf8').replace(/^﻿/, '').replace(/\r\n/g, '\n').trimEnd();
      const key = createHash('sha256').update(text).digest('hex');
      const entry = byText.get(key) ?? { text, users: new Set() };
      entry.users.add(`${r.name} ${r.version} (${f})`);
      byText.set(key, entry);
    }
  }
  return { byText, none };
}

// SPDX standard texts (scripts/license-templates, from spdx/license-list-data).
const TEMPLATE_ORDER = ['MIT', 'BSD-3-Clause', 'Zlib', 'Apache-2.0', 'MPL-2.0', 'Unlicense'];
const template = (id) => readFileSync(join(ROOT, 'scripts', 'license-templates', `${id}.txt`), 'utf8').replace(/\r\n/g, '\n').trimEnd();

function writeLicenseTexts(rust, npm) {
  const out = [
    'Third-party license texts: Rust crates and npm packages',
    '========================================================',
    '',
    'The license, copyright and notice files published with each Rust crate and npm package',
    'that tuldMi ships (generated by scripts/gen-notices.mjs; THIRD_PARTY_NOTICES.md lists them',
    'all with their license and repository). Identical texts appear once, under every package',
    'that publishes them. For a package that publishes no license file, the standard text of',
    'the license it declares follows, with its declared authors as the copyright holders.',
    '',
  ];
  let missing = [];
  for (const [title, rows] of [['Rust crates', rust], ['npm packages', npm]]) {
    const { byText, none } = licenseTexts(rows);
    out.push('', '#'.repeat(78), `# ${title}`, '#'.repeat(78));
    const entries = [...byText.values()].sort((a, b) => [...a.users][0].localeCompare([...b.users][0], 'en'));
    for (const e of entries) {
      out.push('', '-'.repeat(78), ...[...e.users].sort((a, b) => a.localeCompare(b, 'en')), '-'.repeat(78), '', e.text);
    }
    // Packages that publish no license file: the standard text of the license they declare (the
    // first we have a template for, in this order), with their declared authors as the holders.
    for (const r of none) {
      const ids = String(r.license).split(/\s+OR\s+|\//).map((s) => s.trim().replace(/[()]/g, ''));
      const id = TEMPLATE_ORDER.find((t) => ids.includes(t));
      const holders = r.authors?.length ? r.authors.join(', ') : `the ${r.name} authors`;
      out.push('', '-'.repeat(78), `${r.name} ${r.version} (declared ${licOf(r)}; publishes no license file)`, '-'.repeat(78), '');
      if (!id) {
        out.push(`License: ${licOf(r)}. Repository: ${r.repository || 'none'}.`);
        missing.push(`${r.name}@${r.version}`);
      } else if (id === 'Apache-2.0') {
        out.push(`Copyright ${holders}. Licensed under the Apache License 2.0: see Apache-2.0.txt.`);
      } else {
        out.push(template(id).replace(/<year> <copyright holders>|<year> <owner>|<year> <copyright holder>/g, holders));
      }
    }
  }
  const path = join(ROOT, 'src-tauri', 'licenses', 'third-party-licenses.txt');
  writeFileSync(path, out.join('\n') + '\n', 'utf8');
  return { path, missing };
}

// ---------- output ----------
const cell = (s) => String(s ?? '').replace(/\|/g, '\\|').replace(/\s+/g, ' ').trim();
const byName = (a, b) => a.name.localeCompare(b.name, 'en') || a.version.localeCompare(b.version, 'en');
const licOf = (p) => (isMissing(p.license) ? MISSING : p.license);

function table(rows) {
  const lines = ['| Name | Version | License | Repository |', '| --- | --- | --- | --- |'];
  for (const r of rows) {
    lines.push(`| ${cell(r.name)} | ${cell(r.version)} | ${cell(licOf(r))} | ${cell(r.repository)} |`);
  }
  return lines.join('\n');
}

function main() {
  const rust = rustPackages().sort(byName);
  const npm = npmPackages().sort(byName);

  // License summary (combined), with per-ecosystem counts.
  const counts = new Map();
  for (const [kind, rows] of [['rust', rust], ['npm', npm]]) {
    for (const r of rows) {
      const lic = licOf(r);
      const c = counts.get(lic) ?? { total: 0, rust: 0, npm: 0 };
      c.total++;
      c[kind]++;
      counts.set(lic, c);
    }
  }
  const summaryRows = [...counts.entries()].sort(
    (a, b) => b[1].total - a[1].total || a[0].localeCompare(b[0], 'en'),
  );

  const md = `# Third-Party Notices

tuldMi is released under the MIT License (see [LICENSE](LICENSE)). The third-party
components listed below are distributed under their own licenses, which are
reproduced or referenced by the upstream projects linked here. The license texts
of the native libraries and shipped models are in
[src-tauri/licenses/](src-tauri/licenses/README.txt), and the installed app carries
them, with this file, in its \`licenses\` folder. Model weights downloaded by the app
are not covered by this file; see [docs/MODELS.md](docs/MODELS.md).

## Native libraries

These C/C++ libraries are compiled into tuldMi. Cargo metadata does not report
their licenses, so they are listed explicitly.

- whisper.cpp / ggml: MIT, https://github.com/ggml-org/whisper.cpp
- libopus: BSD-3-Clause, https://opus-codec.org
- SQLite: Public domain, https://sqlite.org

Compiled into \`tuldmi-speakers\` (speaker separation), from sherpa-onnx's prebuilt
no-TTS static libraries (no espeak-ng or other text-to-speech code):

- sherpa-onnx: Apache-2.0, https://github.com/k2-fsa/sherpa-onnx
- ONNX Runtime: MIT, https://github.com/microsoft/onnxruntime (and the libraries it
  bundles, listed in its ThirdPartyNotices.txt)
- kaldi-native-fbank: Apache-2.0, https://github.com/csukuangfj/kaldi-native-fbank
- kaldi-decoder: Apache-2.0, https://github.com/k2-fsa/kaldi-decoder
- OpenFst (via kaldifst): Apache-2.0, https://www.openfst.org
- simple-sentencepiece: Apache-2.0, https://github.com/pkufool/simple-sentencepiece
- hclust-cpp (fastcluster): BSD-2-Clause, https://github.com/csukuangfj/hclust-cpp
- Darts-clone (via simple-sentencepiece): BSD-2-Clause, https://github.com/s-yata/darts-clone
- Eigen: MPL-2.0, https://eigen.tuxfamily.org (text in ONNX Runtime's ThirdPartyNotices.txt)
- nlohmann/json: MIT, https://github.com/nlohmann/json (text in ONNX Runtime's ThirdPartyNotices.txt)
- KISS FFT: BSD-3-Clause, https://github.com/mborgerding/kissfft

## Rust crates (${rust.length})

${table(rust)}

## npm packages (${npm.length})

${table(npm)}

## License summary

| License expression | Total | Rust | npm |
| --- | --- | --- | --- |
${summaryRows.map(([lic, c]) => `| ${cell(lic)} | ${c.total} | ${c.rust} | ${c.npm} |`).join('\n')}
`;

  writeFileSync(join(ROOT, 'THIRD_PARTY_NOTICES.md'), md, 'utf8');
  const texts = writeLicenseTexts(rust, npm);
  console.log(`Wrote ${texts.path}`);
  if (texts.missing.length) console.log(`No license file published (declared license listed): ${texts.missing.length}`);

  console.log(`Rust packages: ${rust.length}`);
  console.log(`npm packages: ${npm.length}`);
  console.log(`Wrote ${join(ROOT, 'THIRD_PARTY_NOTICES.md')}`);
  console.log('\nFlagged licenses:');
  const flagged = [...rust, ...npm].filter((p) => isMissing(p.license) || FLAG_RE.test(p.license));
  for (const p of flagged) {
    console.log(`${p.name}@${p.version}: ${licOf(p)}`);
  }
  if (!flagged.length) console.log('(none)');
}

main();
