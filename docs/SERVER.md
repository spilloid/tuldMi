# Homelab transcription

The desktop owns recording, the durable queue, transcripts, playback and retention. Server
processing is an explicit opt-in under Settings → Transcription. It uploads **existing queued
recordings and future speech**. Disabling it returns to the selected local model.

## Target service

- Endpoint: `https://tuldmi.spillerstech.us/v1/audio/transcriptions`
- Authentik issuer: `https://auth.spillerstech.us/application/o/tuldmi/`
- Public OAuth client ID / API audience: `tuldmi-desktop`
- Worker: `server/`, FastAPI + faster-whisper, CPU int8, `base.en`, four threads.
- Kubernetes: `deploy/homelab/`, using homelab_ac's nginx, `letsencrypt-cloudflare` and Longhorn.

The homelab's documented GPUs are Intel iGPUs. This worker uses CPU; it does not request CUDA.
Measure throughput on the target node before promising live transcription. Replicas can serve
multiple desktops; one desktop currently uploads one clip at a time, so more replicas do not
parallelize a single desktop's backlog. A smaller model or faster node can improve that rate.
Multiple replicas on different nodes also need separate model caches or RWX storage; the staged
single-replica Deployment uses a ReadWriteOnce PVC and Recreate upgrades.

## Authentik

1. Create a dedicated OAuth2/OIDC provider and application with slug `tuldmi`. Use a **public**
   client, ID `tuldmi-desktop`, an RSA signing key, per-provider issuer mode, and `openid` and
   `offline_access` scopes. Enable device-code and refresh-token grants.
2. Create the device-code stage/flow and bind it to the provider. Authentik does not ship a
   default device-code flow. Bind the application to the intended users/group and retain the
   normal login/MFA and authorization policies.
3. Check discovery at
   `https://auth.spillerstech.us/application/o/tuldmi/.well-known/openid-configuration`.
   Match the manifest's issuer, JWKS URI and audience to the provider and issued access token.
   The API accepts RS256 access tokens with `iss`, `aud`, `sub`, `iat`, `exp` and `openid` scope.
   Wrong issuers/audiences, expired tokens and tokens without access scopes are rejected.
4. Do **not** put browser forward-auth on the transcription API: the API verifies bearer JWTs
   itself. Login redirects cannot authenticate a background upload.
5. In the desktop settings, enable server uploads and choose **Sign in with Authentik**. Enter
   the displayed device code in the opened browser and approve. Polling respects the provider's
   interval, `slow_down` and expiry; refresh tokens renew sessions while the app runs.

Tokens remain in memory only, never settings, SQLite, logs or exports. **Sign in again after
restarting.** Persistent secure credential storage and account/session management are future
work. This flow still needs acceptance testing against the live homelab provider.

References: [Authentik device flow](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/device_code),
[OAuth provider](https://docs.goauthentik.io/add-secure-apps/providers/oauth2/),
[faster-whisper](https://github.com/SYSTRAN/faster-whisper).

## Stage for homelab_ac

```powershell
docker build -t ghcr.io/spilloid/tuldmi-worker:0.6.0 ./server
kubectl kustomize ./deploy/homelab
```

Publish the reviewed image separately and pin its digest before enabling Flux. The manifest tag
is a planned artifact, not a claim that an image exists. Move these resources into
`homelab_ac/apps/tuldmi/base`, add its prod overlay and Flux entry, and configure DNS/edge routing
using that repository's review process. **No cluster, DNS, registry or Authentik configuration is
changed by this source patch.**

The pod runs unprivileged with a read-only root, a model-cache PVC and memory-backed temporary
uploads. First startup downloads model weights; readiness waits for model loading. Only the
exact transcription API path is public; health/readiness stay cluster-only. Use one Uvicorn
process per pod, since admission limits and loaded models are per-process.

One request runs at a time; busy requests get 503 immediately. Requests are bounded to 10 MiB
and 0.1–300 seconds of mono 16 kHz PCM WAV. The worker returns segment timestamps and no inferred
speaker identities. It keeps no audio archive or transcript database, closes temporary uploads
after each request and disables HTTP access logs. The operator must also control ingress/host
logging and backups. The desktop cannot enforce another server's retention policy.

## Queue health

The timeline shows waiting speech duration and the oldest unfinished recording, with **Review
oldest**. Saved audio is playable while transcripts wait. **Transcribe next** moves a waiting
clip ahead of other queued work; the current clip finishes first.

Service time uses the last 32 successful, non-energy-gated clips, including decode/upload time.
At least three completions are required. Incoming speech is measured across **both** audio
tracks over up to five minutes (minimum 30 seconds). If queued work needs `W` processing
milliseconds and new work occupies fraction `u` of capacity, catch-up is `W / (1 - u)`. At
`u >= 1`, the app says **Falling behind** and withholds a finite catch-up estimate. Warmup,
loading and blocked processing also have no ETA. Paused capture estimates draining existing
work. Model/server changes reset service measurements. The running clip is counted in full.

Upload failures requeue audio without exhausting its retry budget, with exponential backoff
capped at 60 seconds. New recordings cannot bypass backoff. Settings changes, sign-in and
shutdown can interrupt the wait. Corrupt local files still follow normal bounded retry behavior.
Retention protects queued, running and failed recordings.

## Validate

```powershell
npm.cmd run typecheck
npm.cmd test
cargo test -p tuldmi-engine
python -m venv .venv
.venv/Scripts/python -m pip install -r server/requirements.txt httpx
cd server
../.venv/Scripts/python -m unittest -v
```

Deployment acceptance requires real Authentik approval/refresh, TLS/DNS, authenticated uploads
and a timed dual-track workload on the target node. Verify that noon conversations become
reviewable while capture continues; an ETA alone does not establish sufficient capacity.

Current 0.6.0 Windows validation: production frontend build and 74 frontend tests pass. Release
engine validation passes 110 regular tests plus all 3 installed-model tests (113 total), including
real Vulkan inference, token timing and shutdown/recovery. Workspace clippy/all-targets passes
with warnings denied. Desktop/MCP/speaker tests add 21 passing tests (134 native total). See GPU.md for fresh target-machine measurements and CPU fallback evidence.

The unchanged Python worker previously passed four HTTP/auth tests and a real CPU tiny.en smoke
using the public speech fixture. That smoke mocks Authentik signing-key lookup, not inference.
Kubernetes manifests are rendered locally only. The Docker image has not been built/published,
and no cluster, DNS or live Authentik/OpenAI integration is claimed validated. The macOS workflow
now builds Metal by default and checks real speech when the runner exposes a GPU;
The owner reports the app worked on their ARM64 Mac; no independent Mac benchmark or live remote-service acceptance is inferred.
