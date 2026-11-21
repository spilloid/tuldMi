"""Account-scoped recordings. Audio, metadata and the search index commit together."""
import json
import os
import sqlite3
import time
from contextlib import contextmanager
from pathlib import Path
from uuid import UUID

from fastapi import HTTPException

SCHEMA = """
CREATE TABLE IF NOT EXISTS clips (
    owner TEXT NOT NULL, id TEXT NOT NULL, started INTEGER NOT NULL, device TEXT NOT NULL,
    received INTEGER NOT NULL DEFAULT (unixepoch()),
    source TEXT NOT NULL, transcript TEXT NOT NULL, result TEXT NOT NULL, audio BLOB NOT NULL,
    UNIQUE(owner, id)
);
CREATE INDEX IF NOT EXISTS expiry ON clips(received);
CREATE INDEX IF NOT EXISTS timeline ON clips(owner, started DESC, id DESC);
CREATE VIRTUAL TABLE IF NOT EXISTS words USING fts5(transcript);
"""


@contextmanager
def database():
    directory = os.environ.get("TULDMI_LIBRARY_DIR")
    if not directory:
        raise HTTPException(503, "Shared library storage is unavailable")
    db = sqlite3.connect(Path(directory) / "library.sqlite3", timeout=10)
    db.row_factory = sqlite3.Row
    try:
        with db:
            yield db
    finally:
        db.close()


def initialize():
    if os.environ.get("TULDMI_LIBRARY_DIR"):
        with database() as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.executescript(SCHEMA)


def retention_days():
    days = int(os.environ.get("TULDMI_LIBRARY_RETENTION_DAYS", "0"))
    if days < 0:
        raise ValueError("TULDMI_LIBRARY_RETENTION_DAYS must be zero or positive")
    return days


def expire(days):
    if not days:
        return
    cutoff = int(time.time()) - days * 86400
    while True:
        with database() as db:
            db.execute("BEGIN IMMEDIATE")
            ids = db.execute("SELECT rowid FROM clips WHERE received < ? LIMIT 32", (cutoff,)).fetchall()
            if not ids:
                return
            db.executemany("DELETE FROM words WHERE rowid=?", ids)
            db.executemany("DELETE FROM clips WHERE rowid=?", ids)


def metadata(raw):
    try:
        item = json.loads(raw)
        clip = str(UUID(item["id"]))
        started = item["started"]
        device = item["device"].strip()
        source = item["source"]
        if (type(started) is not int or not 0 <= started <= 253402300799999
                or not 1 <= len(device) <= 64 or source not in ("mic", "system")):
            raise ValueError()
        return clip, started, device, source
    except (AttributeError, KeyError, ValueError, TypeError):
        raise HTTPException(422, "Invalid clip metadata") from None


def previous(owner, clip):
    with database() as db:
        row = db.execute("SELECT result FROM clips WHERE owner=? AND id=?", (owner, clip)).fetchone()
        return json.loads(row[0]) if row else None


def save(owner, item, result, audio):
    with database() as db:
        saved = db.execute("""INSERT INTO clips(owner,id,started,device,source,transcript,result,audio)
                            VALUES (?,?,?,?,?,?,?,?) ON CONFLICT(owner,id) DO NOTHING""",
                           (owner, *item, result["text"], json.dumps(result), audio))
        if saved.rowcount:
            db.execute("INSERT INTO words(rowid,transcript) VALUES (?,?)", (saved.lastrowid, result["text"]))
            return result
        existing = db.execute("SELECT result FROM clips WHERE owner=? AND id=?", (owner, item[0])).fetchone()
        return json.loads(existing[0])


def timeline(owner, query, before, before_id):
    conditions = ["owner=?"]
    args = [owner]
    if before is not None:
        conditions.append("(started,id) < (?,?)")
        args.extend([before, before_id])
    if query.strip():
        conditions.append("rowid IN (SELECT rowid FROM words WHERE words MATCH ?)")
        args.append('"' + query.strip().replace('"', '""') + '"')
    with database() as db:
        rows = db.execute("SELECT id,started,device,source,transcript FROM clips WHERE "
                          + " AND ".join(conditions) + " ORDER BY started DESC,id DESC LIMIT 51", args).fetchall()
        return {"clips": [dict(row) for row in rows[:50]], "more": len(rows) > 50}


def recording(owner, clip):
    with database() as db:
        row = db.execute("SELECT audio FROM clips WHERE owner=? AND id=?", (owner, clip)).fetchone()
        if row is None:
            raise HTTPException(404, "Recording unavailable")
        return row[0]


def remove(owner, clip):
    with database() as db:
        db.execute("DELETE FROM words WHERE rowid IN (SELECT rowid FROM clips WHERE owner=? AND id=?)", (owner, clip))
        db.execute("DELETE FROM clips WHERE owner=? AND id=?", (owner, clip))
