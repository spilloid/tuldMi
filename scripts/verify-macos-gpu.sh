#!/usr/bin/env bash
# Explicit, local-only fixture validation. Never opens the capture DB or private recordings.
set -euo pipefail
[[ "$(uname -s)" == Darwin ]] || { echo 'Run this check on macOS.' >&2; exit 2; }
mode="${1:-auto}"
[[ "$mode" == auto || "$mode" == require-metal ]] || { echo 'Use auto or require-metal.' >&2; exit 2; }
repo="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo"
out="${TULDMI_GPU_VALIDATION_DIR:-$repo/scratch/gpu-validation}"
mkdir -p "$out"
model="${TULDMI_TEST_MODEL:-$repo/scratch/gpu-validation/ggml-base.en.bin}"
expected=a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002
if [[ ! -f "$model" ]]; then
    mkdir -p "$(dirname "$model")"
    curl --fail --location --retry 3 --connect-timeout 15 \
        https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin -o "$model"
fi
actual="$(shasum -a 256 "$model" | awk '{print $1}')"
[[ "$actual" == "$expected" ]] || { echo 'Base.en model SHA-256 mismatch.' >&2; exit 1; }
export TULDMI_TEST_MODEL="$model"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CMAKE_BUILD_PARALLEL_LEVEL="${CMAKE_BUILD_PARALLEL_LEVEL:-2}"

# CPU fallback remains operational even in a binary built with native Metal kernels.
env -u TULDMI_EXPECT_VULKAN TULDMI_FORCE_CPU=1 TULDMI_EXPECT_BACKEND=CPU \
    cargo test --locked -p tuldmi-engine --features whisper-gpu --test whisper_backend -- \
    --ignored --nocapture --test-threads=1 2>&1 | tee "$out/cpu.log"

# Ordinary hosted macOS VMs may not expose a Metal device. Distinguish unavailable
# hardware from compiler failures, and never call a CPU fallback a Metal pass.
set +e
swift -e 'import Metal; import Darwin; guard let device = MTLCreateSystemDefaultDevice() else { exit(3) }; print(device.name)' \
    > "$out/metal-device.txt" 2> "$out/metal-probe-error.txt"
probe=$?
set -e
case "$probe" in
    0)
        env -u TULDMI_FORCE_CPU -u TULDMI_EXPECT_VULKAN TULDMI_EXPECT_BACKEND=Metal \
            cargo test --locked -p tuldmi-engine --features whisper-gpu --test whisper_backend -- \
            --ignored --nocapture --test-threads=1 2>&1 | tee "$out/metal.log"
        echo 'CPU=passed; Metal=verified with real speech and token timing' | tee "$out/result.txt"
        ;;
    3)
        # Exercise automatic fallback too, with GPU requested and no CPU override.
        env -u TULDMI_FORCE_CPU -u TULDMI_EXPECT_VULKAN TULDMI_EXPECT_BACKEND=CPU \
            cargo test --locked -p tuldmi-engine --features whisper-gpu --test whisper_backend -- \
            --ignored --nocapture --test-threads=1 2>&1 | tee "$out/no-device.log"
        echo 'CPU=passed; automatic no-device fallback=passed; Metal=UNVERIFIED (no device)' | tee "$out/result.txt"
        if [[ "$mode" == require-metal ]]; then
            echo 'Metal hardware is required for this run.' >&2
            exit 1
        fi
        echo '::warning::Metal inference was not tested: this runner exposes no Metal device.'
        ;;
    *)
        cat "$out/metal-probe-error.txt" >&2
        exit "$probe"
        ;;
esac
