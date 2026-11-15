"""Run real inference through the HTTP API on the repo's non-private speech fixture.

Downloads the selected model on first run. Authentik's signing key is mocked for this local
check; run the real identity-provider acceptance steps in docs/SERVER.md before deployment.
"""
from pathlib import Path
from time import perf_counter
from unittest.mock import patch
from test_app import WorkerTests
from fastapi.testclient import TestClient
import app as worker

WorkerTests.setUpClass()
test = WorkerTests()
fixture = Path(__file__).resolve().parents[1] / "crates/engine/tests/fixtures/speech.wav"
with patch.object(worker.keys, "get_signing_key_from_jwt", return_value=type("Key", (), {"key": test.key.public_key()})()):
    with TestClient(worker.app) as http:
        started = perf_counter()
        response = http.post("/v1/audio/transcriptions", headers={"Authorization": "Bearer " + test.token()},
                             files={"file": ("speech.wav", fixture.read_bytes(), "audio/wav")},
                             data={"model": worker.MODEL, "language": "en", "response_format": "verbose_json"})
        elapsed = perf_counter() - started
        assert response.status_code == 200, response.text
        body = response.json()
        assert body["text"].strip() and body["segments"], body
        assert all(0 <= s["start"] <= s["end"] for s in body["segments"]), body
        print({"model": worker.MODEL, "inference_seconds": round(elapsed, 3), "segments": len(body["segments"]), "fixture_text": body["text"]})
