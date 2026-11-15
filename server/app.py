"""Stateless CPU transcription worker. The desktop owns durable scheduling and retries."""
import asyncio
from contextlib import asynccontextmanager
import io
import os
import threading
import wave

from fastapi import FastAPI, HTTPException, Request
from fastapi.responses import JSONResponse
import jwt
from jwt import PyJWKClient

MODEL = os.getenv("TULDMI_MODEL", "base.en")
ISSUER = os.environ.get("TULDMI_OIDC_ISSUER", "https://auth.spillerstech.us/application/o/tuldmi/")
AUDIENCE = os.getenv("TULDMI_OIDC_AUDIENCE", "tuldmi-desktop")
JWKS = os.getenv("TULDMI_OIDC_JWKS", "https://auth.spillerstech.us/application/o/tuldmi/jwks/")
MAX_BYTES = 10 * 1024 * 1024
UPLOAD_TIMEOUT_SECONDS = 30.0
slots = threading.BoundedSemaphore(int(os.getenv("TULDMI_CONCURRENCY", "1")))
keys = PyJWKClient(JWKS, cache_keys=True, timeout=10)


def authorize(header: str):
    scheme, _, token = header.partition(" ")
    if scheme.lower() != "bearer" or not token or len(token) > 16_384:
        raise HTTPException(401, "Sign in with Authentik")
    try:
        key = keys.get_signing_key_from_jwt(token)
        claims = jwt.decode(token, key.key, algorithms=["RS256"], audience=AUDIENCE,
                            issuer=ISSUER, options={"require": ["exp", "iat", "iss", "aud", "sub"]})
        # Access tokens only. Authentik ID tokens have no scope claim.
        if "openid" not in claims.get("scope", "").split():
            raise HTTPException(403, "Missing transcription access scope")
        return claims
    except jwt.PyJWTError:
        raise HTTPException(401, "Invalid or expired Authentik token") from None


@asynccontextmanager
async def lifespan(app):
    from faster_whisper import WhisperModel
    app.state.model = await asyncio.to_thread(
        WhisperModel, MODEL, device="cpu", compute_type="int8",
        cpu_threads=int(os.getenv("TULDMI_CPU_THREADS", "4")),
        num_workers=int(os.getenv("TULDMI_CONCURRENCY", "1")),
        download_root=os.getenv("HF_HOME", "/models"),
    )
    yield
    app.state.model = None


app = FastAPI(lifespan=lifespan, docs_url=None, redoc_url=None, openapi_url=None)


@app.get("/healthz")
def health():
    return {"status": "ok"}


@app.get("/readyz")
def ready():
    if getattr(app.state, "model", None) is None:
        raise HTTPException(503, "Model loading")
    return {"status": "ready"}


def infer(data: bytes, language: str | None):
    # Accept the desktop's bounded PCM WAV only; no remote URLs or arbitrary media probing.
    import numpy as np
    try:
        with wave.open(io.BytesIO(data), "rb") as audio:
            if (audio.getnchannels(), audio.getsampwidth(), audio.getframerate()) != (1, 2, 16_000):
                raise ValueError()
            if not 1600 <= audio.getnframes() <= 16_000 * 300:
                raise ValueError()
            expected = audio.getnframes() * 2
            pcm = audio.readframes(audio.getnframes())
            if len(pcm) != expected:
                raise ValueError()
    except (wave.Error, EOFError, ValueError):
        raise HTTPException(422, "Expected mono 16 kHz PCM WAV, 0.1 to 300 seconds") from None
    samples = np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0
    segments, info = app.state.model.transcribe(
        samples, language=language, beam_size=1, condition_on_previous_text=False,
        vad_filter=False, word_timestamps=False,
    )
    result = [{"start": s.start, "end": s.end, "text": s.text.strip()} for s in segments if s.text.strip()]
    return {"text": " ".join(s["text"] for s in result), "language": info.language, "segments": result}


@app.post("/v1/audio/transcriptions")
async def transcribe(request: Request):
    # Authorize and admit before parsing uploads. Busy requests get 503 immediately:
    # no growing in-memory server queue, and the desktop retains the recording.
    await asyncio.to_thread(authorize, request.headers.get("authorization", ""))
    if getattr(app.state, "model", None) is None or not slots.acquire(blocking=False):
        return JSONResponse({"detail": "Worker busy"}, status_code=503, headers={"Retry-After": "5"})
    try:
        # Bound the entire multipart request before parsing. The parser's temporary upload
        # file is closed by its context manager and lives on the pod's memory-backed /tmp.
        body = bytearray()
        try:
            # Absolute body deadline: a trickling upload cannot retain admission forever.
            async with asyncio.timeout(UPLOAD_TIMEOUT_SECONDS):
                async for chunk in request.stream():
                    body.extend(chunk)
                    if len(body) > MAX_BYTES:
                        raise HTTPException(413, "Audio upload too large")
        except TimeoutError:
            raise HTTPException(408, "Audio upload timed out") from None
        async def receive():
            return {"type": "http.request", "body": bytes(body), "more_body": False}
        bounded = Request(request.scope, receive)
        async with bounded.form(max_files=1, max_fields=4) as form:
            upload = form.get("file")
            if not hasattr(upload, "read") or form.get("model") != MODEL:
                raise HTTPException(422, "Supply a WAV file and the configured model")
            language = form.get("language") or None
            if language is not None and (not isinstance(language, str) or len(language) > 8):
                raise HTTPException(422, "Invalid language")
            data = await upload.read(MAX_BYTES + 1)
            try:
                # Shield inference from disconnect cancellation: keep the admission slot
                # until native computation actually finishes, including during shutdown.
                task = asyncio.create_task(asyncio.to_thread(infer, data, language))
                try:
                    return await asyncio.shield(task)
                except asyncio.CancelledError:
                    await task
                    raise
            except HTTPException:
                raise
            except Exception:
                # Do not put audio, transcripts, token claims, or model errors in logs.
                raise HTTPException(503, "Transcription unavailable") from None
    finally:
        slots.release()
