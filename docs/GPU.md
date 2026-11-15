# Local transcription acceleration

The desktop application's default `gpu` Cargo feature enables whisper.cpp's native
Metal backend on macOS and Vulkan backend on Windows. Apple Silicon's integrated
GPU is used through Metal; supported Intel integrated GPUs use their Vulkan driver
on Windows. Target-specific dependencies keep each platform's native SDK separate.
The engine crate alone retains its portable CPU default; opt in with `whisper-gpu`.
Legacy `vulkan` / `whisper-vulkan` feature names remain aliases for existing commands.

## macOS build and validation

Apple Silicon builds select Metal automatically, including normal development and
the macOS app/DMG workflow. Xcode command line tools and CMake are required to build;
Vulkan and the LunarG SDK are not. whisper.cpp embeds its Metal shader source into
the executable; bundle verification checks the Metal framework and embedded section,
so users do not need an external shader file. This enables Metal GPU offload, not
Core ML or Apple Neural Engine execution. The supported macOS target remains
Apple Silicon on macOS 15.4+.

```sh
cargo build -p tuldmi --release
# Optional CPU-only diagnostic build:
cargo build -p tuldmi --release --no-default-features
# Real public speech fixture, CPU comparison and mandatory observed Metal backend:
bash scripts/verify-macos-gpu.sh require-metal
```

The validation script downloads hash-checked `base.en` weights if needed (about
148 MB), uses only the checked-in public fixture, and records metadata/timings under
`scratch/gpu-validation`. It checks recognition, token bytes and timestamps as well
as the actual backend. `TULDMI_TEST_MODEL` can point to existing matching weights.

CI runs this check in `auto` mode: a runner without a Metal device reports Metal
as **unverified**, while still checking CPU inference, compilation and bundling.
An explicit workflow dispatch can select the metered `macos-15-xlarge` GPU runner
and `require_metal=true`; ordinary runs keep their existing runner. Validation logs
are uploaded separately from the disk image. Local Windows checks verify Cargo
feature routing for both targets, but do not establish Mac performance. The owner reports successful operation on their ARM64 Mac and authorizes release
on that basis. This is user-reported hardware testing; no specific backend label,
timing, permission-denial coverage or independently measured Mac speedup is inferred.

## Windows build

Install the [official LunarG Vulkan SDK](https://vulkan.lunarg.com/sdk/home#windows),
in addition to the normal Rust/MSVC/LLVM prerequisites and Ninja on `PATH`. Set `VULKAN_SDK` to its
root and add its `Bin` directory to the current shell's `PATH`. The SDK supplies
Vulkan headers, the import library, and `glslc` for compiling the embedded shaders.
No SDK or shader compiler is required on the end user's machine. Preserve the
workspace `.cargo/config.toml` static-CRT rustflags and CMake toolchain: the 0.4
speaker worker's pinned sherpa-onnx libraries require `/MT`, and no binary should
acquire a Visual C++ redistributable dependency. The workspace also sets
`CMAKE_POLICY_DEFAULT_CMP0091=NEW`: without it, whisper.cpp's older CMake policy
baseline lets Ninja append legacy `/MD` defaults after `/MT`. Final executable
imports must be checked, not inferred from the requested runtime flag.

```powershell
$env:VULKAN_SDK = 'C:\VulkanSDK\1.4.328.1'
$env:PATH = "$env:VULKAN_SDK\Bin;$env:PATH"
$env:CARGO_TARGET_DIR = 'C:\tuldmi-b060'
$env:CMAKE_GENERATOR = 'Ninja'
$env:CARGO_BUILD_JOBS = '2'
$env:CMAKE_BUILD_PARALLEL_LEVEL = '2'
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
cargo build -p tuldmi --release
```

For a separate CPU-only executable, use `cargo build -p tuldmi --release
--no-default-features`. This variant does not link the Vulkan loader. A Vulkan
executable needs the driver's `vulkan-1.dll` at process startup even when using
CPU fallback; it cannot handle a missing loader from inside Rust.

`./scripts/release.ps1` builds the default Vulkan installer; use
`./scripts/release.ps1 -CpuOnly` for the separate CPU installer. Either accepts
the existing `-Sign` option. The script preserves distinct `-vulkan-setup.exe`
and `-cpu-setup.exe` copies and their SHA-256 files in the NSIS bundle directory.
It does not publish either artifact. The existing 0.4 packaging flow is retained:
it builds both MCP and speaker sidecars, uses `tauri.bundle.conf.json`, and checks
their versions. Sidecar output is located through Cargo metadata so an external
`CARGO_TARGET_DIR` works. Do not replace this flow with a bare app-only bundle.

Use a short `CARGO_TARGET_DIR`: the upstream nested shader-generator project
exceeds Windows path-length limits under deep repository checkouts, producing
misleading `No CMAKE_C_COMPILER could be found` / MSB6003 errors. The release
script defaults to `C:\tuldmi-build` (override with `-TargetDir` or
`CARGO_TARGET_DIR`), placing installers under `release\bundle\nsis` there.
The GPU release build defaults to the Ninja CMake generator with MSVC, avoiding
MSBuild's nested compiler-test `.tlog` path failures. CPU-only Tauri CLI builds
pass the Cargo flag after the separator: `npx tauri build -- --no-default-features`.

## Runtime behavior and diagnostics

Model loading and buffer allocation request Metal or Vulkan automatically in GPU builds.
When native initialization returns an error, loading is retried on CPU. The
native backend also uses CPU when no GPU is available. Setting `TULDMI_FORCE_CPU`
before launch forces CPU use for troubleshooting and comparisons (remove the
variable to restore automatic selection).

The backend label uses narrowly filtered native initialization markers followed
by successful inference-buffer allocation. It reports `Metal`, `Vulkan`, `CPU`,
or unknown if those markers are unavailable; enabling the build feature alone is
not proof of an active GPU. A GPU label means offload is initialized, not that every
graph operation runs on the GPU. The log filter never retains or emits recognized text.

CPU inference threads remain bounded to half the logical processors, at most
six, to leave capture headroom. The existing atomic cancellation callback remains
active. Native driver crashes or unrecoverable device failures cannot be repaired
by the initialization fallback; use the CPU variant/override in that case.

## Fixture verification

Use the checked-in speech fixture and existing model weights, never private
recordings. This opt-in test checks that real transcription succeeds and asserts
GPU initialization rather than silently accepting a CPU run:

```powershell
$env:TULDMI_TEST_MODEL = 'C:\path\to\ggml-base.en.bin'
$env:TULDMI_EXPECT_BACKEND = 'Vulkan'
cargo test -p tuldmi-engine --features whisper-vulkan --test whisper_backend -- --ignored --nocapture
Remove-Item Env:TULDMI_EXPECT_BACKEND
$env:TULDMI_FORCE_CPU = '1'
cargo test -p tuldmi-engine --features whisper-vulkan --test whisper_backend -- --ignored --nocapture
Remove-Item Env:TULDMI_FORCE_CPU
```

The test prints backend, load/inference timings, audio duration and segment count,
without printing transcript text. Compare repeated warm runs on the target GPU;
short clips, shader warmup and shared iGPU memory can affect speed. Driver support
and a successful GPU build do not by themselves establish a speedup.

## Verified 0.6.0 fixture results (2026-11-14)

Corrected integration based on v0.4.0, with its token timestamps and speaker
pipeline preserved. Intel Iris Xe, driver 32.0.101.7085 (Vulkan API 1.4.323),
Core i7-1185G7; installed `base.en` weights and the checked-in 6.570-second
fixture. No private capture audio was used.

All 110 regular engine tests and all three opt-in installed-model tests passed
in the release profile with Vulkan. Both Vulkan and forced CPU recognized
`budget` and `thursday` in every fixture round. Tests also verify nonempty token
pieces, positive token spans, bounded timestamps, and exact reconstruction of
segment text from token bytes, as required by speaker separation.

| Backend | Model/buffer initialization | Inference rounds |
| --- | ---: | ---: |
| Vulkan | 0.452 s | 0.429 s, 0.500 s, 0.476 s |
| Forced CPU | 0.316 s | 3.857 s, 4.047 s, 4.272 s |

These measurements used normal process affinity and BelowNormal priority after
compilation finished. The GPU driver cache was already warm from the preceding
correctness run; these are not cold-start timings. That earlier run, restricted
to two logical CPUs, took 7.719 s for its first Vulkan inference and 0.614/0.684 s
for subsequent rounds. No unrelated compiler processes were observed during the
normal-affinity measurements, but the desktop was not an isolated benchmark
machine. Results are workload-specific and do not guarantee a speedup for every
model, clip length, driver, or concurrent workload. Live capture under sustained
GPU load remains a separate end-to-end validation.

Build: whisper-rs 0.16, Vulkan SDK 1.4.328.1, MSVC 14.51, Ninja, release profile,
static CRT, and short target directory. Build jobs were limited to two and the
build process tree to two logical CPUs at BelowNormal priority. Inference used
the provider's normal half-logical-processor CPU thread limit.

Automatic no-device fallback was also verified in the corrected 0.6.0 build:
with process-local `GGML_VK_VISIBLE_DEVICES=' '` and no `TULDMI_FORCE_CPU`, all
three fixture rounds reported `CPU` and passed the same keyword and token
timestamp/byte checks. This verifies fallback with the Vulkan loader present;
it does not remove the GPU binary's loader dependency.

## Metal integration regression checks (2026-11-14)

Local Cargo feature checks pass for Windows Vulkan, Apple Silicon Metal, and
CPU-only builds on both targets. The macOS workflow YAML and all 16 shell steps
parse successfully; the standalone Mac fixture script passes Bash syntax checking.
These are source/build-routing checks, not a Mac compilation or inference result.

The shared runtime changes pass Windows backend-marker tests and real `base.en`
fixture inference with observed `Vulkan` and forced `CPU` backends, including
recognition and token timing/byte checks. Release workspace Clippy passes for all
targets with warnings denied. The wider library run passed 67 tests, failed one
speaker-worker wall-clock watchdog, and ignored one model-dependent test. The
unchanged watchdog passed when rerun alone; the two backend-marker tests also
passed separately. Remaining broad integration-test linking was stopped to limit
machine load; the prior 0.6.0 integration evidence above is retained.

These runs overlapped other machine activity and included priority starvation;
their timing logs are correctness evidence, not new performance measurements.
The owner subsequently reported successful operation on their ARM64 Mac and
authorized release. This is credited as user-tested; no independent Metal benchmark
or detailed permission/restart checklist is inferred. CI separately builds and verifies
the release disk image and records whether its runner exposes a Metal device. Validation logs: `C:/tuldmi-tools/validation/0.6.0-mac-metal`.
