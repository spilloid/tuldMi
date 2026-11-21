# Homelab transcription

The desktop owns capture and its durable upload queue. The optional shared library stores a separate copy of acknowledged speech and its transcript. Server
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
2. Create a Stage Configuration flow requiring authentication and select it as the active brand's device code flow. Authentik does not ship a
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

## Shared timeline and playback

In Settings > Transcription choose **Your server**, check **Keep queued and future speech in my shared library**, enter a distinct laptop name, enable server uploads, and sign in with the same Authentik account on each laptop. Open **Shared timeline** in the navigation rail to search transcripts and play or delete shared clips. Each clip shows the original capture time, source and laptop. Use Refresh for new arrivals and Older recordings for more history. Completed local history is outside the upload queue.

Sharing sends speech to `/v1/library/transcriptions`. The worker stores the WAV, transcript, capture metadata and search index in a SQLite transaction on the library PVC. A receipt for the original event UUID allows the desktop to finish its job. A retry after a lost response returns the existing result. The normal transcription endpoint continues to use temporary audio.

Local retention sets the audio cache on each laptop. Pending uploads survive automatic cleanup. Quiet clips and music skipped by the desktop follow their local grace periods. `TULDMI_LIBRARY_RETENTION_DAYS` controls the shared audio **and** transcript from the date the server receives the clip. Zero keeps them until deletion. When positive, cleanup runs on startup and hourly. Deleting a shared copy removes it from every signed-in laptop's shared view. Local deletions and local cache eviction apply to that laptop.

The single worker uses a Longhorn RWO library claim, separate from its model cache. Keep one replica with Recreate upgrades. At mono 16 kHz PCM, budget about 110 MiB per hour of retained speech across all laptops. Monitor free space. For an independent backup use SQLite online backup or an application-consistent volume backup, then restore into an isolated directory and verify playback. The mounted data and replicas alone do not establish a recoverable off-cluster backup.

The worker verifies Authentik tokens on every library request and scopes all operations to the token subject. Browser forward-auth redirects break background clients. Health and readiness stay cluster-only. The packaged manifest pins the image digest and keeps access logging off at the ingress. Flux deployment and the Authentik blueprint are in HomeLab_ac.

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
