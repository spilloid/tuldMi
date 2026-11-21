import asyncio
import io
import json
import os
import tempfile
from uuid import uuid4
import time
import unittest
import wave
from types import SimpleNamespace
from unittest.mock import patch
from fastapi import HTTPException, Request

from cryptography.hazmat.primitives.asymmetric import rsa
from fastapi.testclient import TestClient
import jwt
import app as worker


class WorkerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.key = rsa.generate_private_key(public_exponent=65537, key_size=2048)

    def token(self, **overrides):
        claims = {"iss": worker.ISSUER, "aud": worker.AUDIENCE, "sub": "user-1",
                  "iat": int(time.time()), "exp": int(time.time()) + 60, "scope": "openid"}
        claims.update(overrides)
        return jwt.encode(claims, self.key, algorithm="RS256", headers={"kid": "test"})

    def setUp(self):
        worker.app.state.model = SimpleNamespace(transcribe=lambda *a, **kw: (
            iter([SimpleNamespace(start=0.0, end=0.1, text="Fixture words")]), SimpleNamespace(language="en")))
        self.client = TestClient(worker.app)
        self.jwks = patch.object(worker.keys, "get_signing_key_from_jwt", return_value=SimpleNamespace(key=self.key.public_key()))
        self.jwks.start()

    def tearDown(self):
        self.jwks.stop()
        self.client.close()

    def upload(self, token=None, data=None, clip=None):
        wav = io.BytesIO()
        with wave.open(wav, "wb") as audio:
            audio.setnchannels(1); audio.setsampwidth(2); audio.setframerate(16_000)
            audio.writeframes(b"\0\0" * 1600)
        return self.client.post("/v1/library/transcriptions" if clip else "/v1/audio/transcriptions",
            headers={"Authorization": "Bearer " + (token or self.token())},
            files={"file": ("speech.wav", wav.getvalue() if data is None else data, "audio/wav")},
            data={"model": worker.MODEL, "response_format": "verbose_json", **({"clip": json.dumps(clip)} if clip else {})})

    def test_missing_and_wrong_audience_or_expired_tokens_are_rejected(self):
        self.assertEqual(self.client.post("/v1/audio/transcriptions").status_code, 401)
        for token in [self.token(aud="other"), self.token(exp=int(time.time()) - 10), self.token(iss="https://other/")]:
            self.assertEqual(self.upload(token).status_code, 401)
        self.assertEqual(self.upload(self.token(scope="")).status_code, 403)

    def test_valid_request_returns_real_model_segments(self):
        response = self.upload()
        self.assertEqual(response.status_code, 200, response.text)
        self.assertEqual(response.json()["segments"][0]["text"], "Fixture words")

    def test_two_laptops_share_durable_audio_only_with_their_account(self):
        with tempfile.TemporaryDirectory() as folder, patch.dict(os.environ, TULDMI_LIBRARY_DIR=folder):
            worker.library.initialize()
            headers = {"Authorization": "Bearer " + self.token()}
            other = {"Authorization": "Bearer " + self.token(sub="another-account")}
            def clips(**params):
                return self.client.get("/v1/library/clips", headers=headers, params=params).json()["clips"]

            a = {"id": str(uuid4()), "started": 1000, "source": "mic", "device": "Work"}
            b = {**a, "id": str(uuid4()), "device": "Travel", "started": 2000, "source": "system"}
            self.assertEqual(self.upload().status_code, 200)
            self.assertEqual(clips(), [])
            for clip in (b, a, a):
                response = self.upload(clip=clip)
                self.assertEqual(response.status_code, 200, response.text)
                self.assertEqual(response.json()["archived"], clip["id"])
            self.assertEqual([c["device"] for c in clips(q="Fixture words")], ["Travel", "Work"])
            self.assertEqual([c["id"] for c in clips(before=2000, before_id=b["id"])], [a["id"]])
            self.assertFalse(clips(q="absent"))
            path = "/v1/library/clips/" + a["id"]
            audio = self.client.get(path + "/audio", headers=headers)
            self.assertEqual(audio.content[:4], b"RIFF")
            self.assertEqual(wave.open(io.BytesIO(audio.content)).getnframes(), 1600)
            self.assertEqual(self.client.get(path + "/audio", headers=other).status_code, 404)
            self.assertEqual(self.client.get("/v1/library/clips", headers=other).json()["clips"], [])
            self.assertEqual(self.client.delete(path, headers=other).status_code, 204)
            self.assertEqual(self.client.get(path + "/audio").status_code, 401)
            worker.library.initialize()  # reopen the existing store
            worker.library.expire(1)  # old capture time, newly offloaded
            self.assertEqual(len(clips()), 2)
            with worker.library.database() as db:
                db.execute("UPDATE clips SET received=0 WHERE id=?", (b["id"],))
            worker.library.expire(1)
            self.assertEqual([c["id"] for c in clips(q="Fixture")], [a["id"]])
            self.assertEqual(self.client.delete(path, headers=headers).status_code, 204)
            self.assertEqual(self.client.get(path + "/audio", headers=headers).status_code, 404)
            self.assertEqual(clips(q="Fixture"), [])

    def test_busy_worker_rejects_without_running_another_inference(self):
        worker.slots.acquire()
        try:
            self.assertEqual(self.upload().status_code, 503)
        finally:
            worker.slots.release()
        self.assertEqual(self.upload().status_code, 200)

    def test_invalid_audio_and_oversized_upload_release_capacity(self):
        self.assertEqual(self.upload(data=b"invalid").status_code, 422)
        with patch.object(worker, "MAX_BYTES", 200):
            self.assertEqual(self.upload().status_code, 413)
        self.assertEqual(self.upload().status_code, 200)

    def test_trickling_upload_hits_absolute_deadline_and_releases_capacity(self):
        async def slow_receive():
            # Frequent progress must not reset the absolute deadline.
            await asyncio.sleep(0.01)
            return {"type": "http.request", "body": b"x", "more_body": True}

        async def run():
            request = Request({"type": "http", "headers": []}, slow_receive)
            with patch.object(worker, "authorize"), patch.object(worker, "UPLOAD_TIMEOUT_SECONDS", 0.04):
                with self.assertRaises(HTTPException) as caught:
                    await asyncio.wait_for(worker.transcribe(request), timeout=1)
            self.assertEqual(caught.exception.status_code, 408)

        asyncio.run(run())
        self.assertEqual(self.upload().status_code, 200)


if __name__ == "__main__":
    unittest.main()
