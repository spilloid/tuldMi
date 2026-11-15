# Cloud transcription cost and API-key setup

Prices checked November 14, 2026. These are estimates for speech-to-text, not audio codec
conversion, an account balance, or a spending limit. tuldMi keeps codec conversion local.

## Eight-hour day

Eight hours is 480 minutes. tuldMi captures microphone and system audio separately, so the
billable amount is their **combined retained duration**, including pre/post-roll and overlap.
Two tracks continuously active for eight hours can therefore upload 960 minutes.

`estimated USD = (microphone minutes + system-audio minutes) × price per minute`

| Model | Listed / estimated USD per minute | 480 uploaded minutes | 960 uploaded minutes |
| --- | ---: | ---: | ---: |
| GPT Transcribe (`gpt-transcribe`, current default) | $0.0045 | $2.16 | $4.32 |
| GPT-4o Mini Transcribe (deprecated) | ~$0.003 | ~$1.44 | ~$2.88 |
| GPT-4o Transcribe / Whisper (deprecated) | ~$0.006 | ~$2.88 | ~$5.76 |
| Claude | Not available for direct audio transcription | N/A | N/A |

The current GPT Transcribe model is billed by audio duration. The GPT-4o figures are published
per-minute estimates for token-billed models; actual usage can vary. The older transcription
models retire February 26, 2027, so GPT Transcribe is the default for new setups.

With speech filtering retaining 25% on each of two tracks: `480 × 2 × 0.25 = 240 minutes`,
which is **$1.08/day** with GPT Transcribe, or **$21.60 across 20 such workdays**. This percentage
is an assumption you can change in Settings. Eight hours of continuous combined speech at the
same rate is $2.16/day; $4.32/day is the two-track continuous scenario before duplicated overlap.
An existing 45-minute queue would add about **$0.20** if uploaded once.

Estimates exclude taxes, retries, provider rounding and already queued audio. The settings
calculator shows an extra estimate for the current queue. Capture outside the assumed window
adds usage. Duplicated audio at clip boundaries can also add billable minutes. A request that completes remotely after a client timeout may be billed again on
retry. Provider usage/billing is the authority; this release does not enforce a daily budget.

Sources: [OpenAI pricing](https://developers.openai.com/api/docs/pricing),
[current file transcription](https://developers.openai.com/api/docs/guides/speech-to-text),
[Claude model capabilities](https://platform.claude.com/docs/en/models/overview),
[Claude audio-input limitation](https://platform.claude.com/docs/en/cli-sdks-libraries/libraries/openai-sdk).

## API-key flow

1. In the desktop app, open Settings → Transcription → Offload transcription → OpenAI.
2. Paste an OpenAI API key and choose **Save API key**. Saving alone does not enable uploads.
   The key is stored in Windows Credential Manager under this app, never in SQLite, settings,
   transcript exports or logs, and is never returned to the frontend after saving.
3. Choose the model, review the cost estimate, then choose **Enable OpenAI uploads**. This sends
   existing queued recordings and future retained speech directly to OpenAI's official HTTPS
   transcription endpoint. The key cannot be redirected to a custom endpoint.
4. **Use local processing** stops future uploads. An already in-flight request can finish.
   **Remove saved key** removes the credential; deleting recording history does not remove it.

The app sends the current model's `languages[]` hints and accepts detected-language JSON.
GPT models preserve clip-level timing here; they do not supply word timing. Legacy Whisper
can return segment timestamps. Mic/Remote source labels remain local metadata; cloud processing
does not imply remote-speaker diarization.

Claude currently accepts text and images, not audio transcription. The existing opt-in MCP
integration can give Claude access to transcript text; that uses the assistant client's separate
billing or subscription and does not replace speech-to-text. This release does not present a
nonworking Claude audio provider or collect an unused Claude key.

Protocol and error handling are tested against a local mock without real keys or paid requests.
A live OpenAI request remains to be checked after the user supplies a key in the desktop app.

Cloud responses have no local token timing. Speaker separation can label whole segments when
a voice clearly dominates, but cannot split a mixed-voice segment into timed words without
those timestamps. Music rejection and speaker processing still run locally. Saved OpenAI keys
in this release require Windows Credential Manager; Authentik server mode is separate.
