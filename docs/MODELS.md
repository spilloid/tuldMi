# Models and licenses

tuldMi's source is MIT. The models it uses are separate works with their own licenses. None of
the transcription models are bundled with the installer; you download one explicitly, on first
run or later in Settings → Transcription.

## Voice activity detection (bundled)

| Model | File | Size | License | Source |
|---|---|---|---|---|
| Silero VAD v6.2.0 (ggml) | `ggml-silero-v6.2.0.bin` | 885 KB | MIT (Silero Team; ggml conversion by ggml-org) | huggingface.co/ggml-org/whisper-vad |

SHA-256 `2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987`. Bundled because it is
tiny and needed to capture anything. If it fails to load, tuldMi falls back to an energy-based
detector and says so.

## Transcription (optional downloads)

Engine: [whisper.cpp](https://github.com/ggml-org/whisper.cpp) (MIT) via whisper-rs (Unlicense),
CPU inference. Weights: OpenAI Whisper (MIT), ggml conversions published by the whisper.cpp
project at `huggingface.co/ggerganov/whisper.cpp`. Sizes and SHA-256 hashes below are checked
after every download.

| Id | File | Size | Languages | Notes | SHA-256 |
|---|---|---|---|---|---|
| `whisper-tiny.en` | ggml-tiny.en.bin | 74 MB | English | fastest, least accurate | `921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f` |
| `whisper-base.en` | ggml-base.en.bin | 141 MB | English | **recommended** default | `a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002` |
| `whisper-base` | ggml-base.bin | 141 MB | ~99 languages | | `60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe` |
| `whisper-small.en` | ggml-small.en.bin | 465 MB | English | more accurate, slower | `c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d` |
| `whisper-large-v3-turbo-q5` | ggml-large-v3-turbo-q5_0.bin | 547 MB | ~99 languages | most accurate, slowest on CPU | `394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2` |

MIT permits redistribution with attribution; tuldMi still downloads on demand to keep the
installer small and to make the choice (and its disk cost) yours.

## Why whisper.cpp and not Parakeet for v0.1.0

NVIDIA Parakeet TDT 0.6B v3 is faster and very accurate, and was evaluated. It was not shipped in
v0.1.0 because:

- Its practical local runtime is ONNX Runtime plus a custom TDT decoder, i.e. a second native
  inference stack beside whisper.cpp (which tuldMi already needs for Silero VAD) — more installer
  weight and more to go wrong on day one.
- The weights are CC-BY-4.0: permissive and redistributable with attribution, but not MIT, so it
  needs its own attribution surface in the UI.
- It supports 25 European languages, versus Whisper's ~99.

The `TranscriptionProvider` trait is the seam for adding it (likely v0.2).

## Not used

No cloud ASR. No speaker-diarization model in v0.1.0 (mic/system source separation only).
