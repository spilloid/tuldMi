# Remote speaker separation (0.4.0)

Status: building on `feat/speakers`. The operator chose this scope on 2026-11-09:

- Separate remote speakers **within each conversation**: Speaker 1, Speaker 2, and so on.
- The user can rename a speaker for that conversation.
- **No voice data is kept** once processing is done.
- Recognizing the same voice across meetings is out of scope. It needs its own consent design.

## What the user sees

- **Remote speech is labelled by speaker.** In a conversation where tuldMi has separated Remote speech, each passage is labelled Speaker 1, Speaker 2, and so on, instead of "Remote". When one detected speech event contains two voices, the transcript is split where the voice changes.
- **Short or unclear speech stays "Remote".** Speech nobody can be sure about keeps the "Remote" label. tuldMi never guesses.
- **Renaming a speaker:** click a speaker's label and name them. The name applies to that speaker in that conversation everywhere: timeline, search, copy, and MCP.
- **Settings › Transcription › Speakers:**
  - an off-by-default switch;
  - a consented download of the two models (46 MB together);
  - **Separate past recordings**, which processes earlier conversations that still have their audio.
- **Me is never split.** It stays Me, because it's your microphone.

## Pipeline

The transcription thread already decodes each event's Ogg Opus file to 16 kHz samples. For a **system** event, when the feature is on and both models are installed, it does this:

1. **Whisper** runs as before, now with token timestamps.
2. **The worker** receives the same samples and returns speaker turns plus one embedding per local speaker. See "Worker process" below.
3. **The tracker** matches each local speaker to the current conversation's voices. This uses in-memory centroids only. See "Tracker" below.
4. **Assignment** splits each Whisper segment where the speaker changes, at token boundaries. Each piece gets a speaker or none, meaning Remote.
5. **One transaction** saves the segments with their `speaker_id`s and sets `audio_events.speakers_state = 'done'`. Then the delete-after-transcription policy runs, unchanged. That's why the audio is still there in step 2.

**If the worker fails** (it crashes, times out, or returns bad output), the transcript saves exactly as in 0.3.0 with `speakers_state = 'failed'`. Transcription never waits on speakers or is lost because of them.

## Worker process: `tuldmi-speakers`

sherpa-onnx's C++ code calls `_Exit()` in about 25 places: bad input, short segments, model errors. A library like that can't run inside the app. So it runs in a sidecar, `crates/speakers`, packaged like `tuldmi-mcp`.

**Protocol:**
- Everything goes over stdin and stdout, little-endian, one request in flight at a time.
- Request: `u32 sample_count`, then that many `f32` samples (16 kHz mono, at most 10 minutes).
- Reply: `u32 len`, then that many bytes of JSON: `{"turns":[{"startMs","endMs","local"}], "voices":[{"local","speechMs","embedding":[…]}]}` or `{"error":"…"}`.

**What the worker does:**
- It appends 1 s of silence. This avoids a known sherpa-onnx abort on speech at the very end of the buffer.
- It separates speakers within the buffer (clustering threshold 0.5, two threads).
- It computes one TitaNet embedding per local speaker that has at least 0.5 s of speech.
- It writes nothing else to stdout. The parent discards its stderr. It never sees text.

**How the engine manages it:**
- It starts the worker on first need. It kills the worker on shutdown and after any protocol error or timeout. The timeout is 30 s plus 2× the audio length, and it covers writing the request too: the write runs on its own thread, so a worker that stops reading can't block the transcription thread.
- It starts a new worker for the next request.
- After 3 consecutive failures it stops for the session, with one notice.

**Builds:**
- The worker links sherpa-onnx's static prebuilt libraries, pinned and hash-checked.
- On Windows those libraries are /MT, so the workspace builds with `+crt-static`.

## Tracker (pure, unit-tested)

The tracker holds a conversation's voices in memory:
- a centroid per voice;
- how much speech each voice has;
- the speaker row each voice has been given, if any.

**A conversation** follows the timeline's rule exactly (`tracker::continues`, the same predicate as `buildTimeline`): an event continues it when it starts on the same local day and less than 3 minutes after the latest end so far, over events of either source. The tracker, the backfill's grouping and the restart lookup all use that one predicate. The transcription thread reports mic events too, so a long stretch of you talking doesn't split the meeting.

**Matching a local speaker:**
- If its embedding's cosine similarity to the best centroid is at least 0.5, it joins that voice, and the centroid becomes the speech-weighted mean.
- Otherwise, with at least 1.5 s of speech, it starts a new voice.
- Otherwise it stays unlabelled.

**Numbering:**
- A voice gets a number, and a `speakers` row with kind `person` and label `Speaker N`, only once it has 10 s of speech. Its earlier segments in the conversation are then relabelled.
- This keeps one-off misdetections from showing up as Speaker 9.

**Out-of-order events** (a retry of an old event) skip separation, leaving `speakers_state` NULL. The backfill can handle them later.

**After a restart** inside a conversation, numbering continues past the highest number that conversation already shows. Each speaker row stores its number (`speakers.ordinal`), so a renamed or deleted speaker's number is never reused, and the lookup is bounded to the conversation. Voices can't be re-linked, because no embeddings were kept. That's the privacy trade-off.

**Tuning** comes from a spike on the AMI Meeting Corpus (CC BY 4.0):

| Meeting | Length | People | Speakers shown | Speech labelled correctly | Left as Remote |
|---|---|---|---|---|---|
| ES2004a | 17 min | 4 | 4 | 98% | 10% |
| IS1009a | 14 min | 4 | 4 | 90% | 5% |

## Backfill

**Separate past recordings** queues conversations in which **every** system event:
- is done;
- still has retained audio;
- has `speakers_state` NULL.

It runs on the transcription thread only when no transcription job is waiting. It processes one conversation at a time, in order, with a fresh tracker. **Nothing is written until the whole conversation is processed**; then its speakers, labels and outcomes are saved in one transaction. A conversation interrupted by quitting or switching off therefore stays eligible, and events separated in the meantime are left alone.

**Existing segments aren't re-transcribed.** Each one goes to the speaker that covers at least 60% of its overlapped speech and at least 30% of its length, and otherwise stays Remote. Without token timestamps there is nothing to split on. Progress is shown in Settings.

## Storage (migration 0003)

```sql
ALTER TABLE transcript_segments ADD COLUMN speaker_id TEXT REFERENCES speakers(id) ON DELETE SET NULL;
ALTER TABLE audio_events ADD COLUMN speakers_state TEXT CHECK (speakers_state IN ('done', 'failed'));
CREATE INDEX transcript_segments_speaker ON transcript_segments(speaker_id);
```

- A segment's `speaker_id` NULL means the event's speaker: Me or Remote.
- `speakers.embedding` stays NULL. It is never written.
- `speakers.ordinal` holds the N of "Speaker N" and survives renames.
- Housekeeping resets the name of a `person` speaker that no segment references any more back to "Speaker N" (after a day, so a row being written can't be swept). The row, with its number and first-shown time, stays, so the number is never reused in that conversation; nothing personal is left in it.
- The restart lookup counts speakers first shown in the conversation and speakers its surviving events still show.
- Live and backfill results carry the settings generation they came from, re-checked under the persist gate right before saving.
- Every re-announcement of a changed event (relabel, rename, backfill) happens under the persist gate, so it can't overtake a deletion.
- `TranscriptSegment` gains `speaker: Option<Speaker>`.
- Search hits and MCP results carry the segment speaker's label.

## MCP

Segments are labelled with their speaker's name. The ceiling is unchanged: a speaker row is Remote content, so it is only visible with `include_remote`.

## Copying a range (same release)

- **On the timeline:** click an event, Shift+click another, then press Ctrl/⌘+C or use the Copy bar that appears. The copied text has one line per speaker run, in the form `[2026-11-09 14:02:11] Speaker 1: …`.
- **Copy conversation:** each conversation header has a Copy conversation button that does the same for the whole conversation, fetching any part of it not loaded yet.

## Privacy

- **Network:** models download only after the user consents, from pinned Hugging Face URLs with SHA-256 checks. Nothing else touches the network.
- **Embeddings:** these are voice fingerprints of other people. They exist only in the app's memory, for the current conversation, and are dropped when the conversation ends (checked while idle too, not only when the next event arrives), when the feature is turned off (checked on every loop of the transcription thread), and at quit. A result that arrives after the feature was turned off is discarded. They are never written to disk or logged.
- **Names** the user types are stored in `speakers.label`. Once no remaining speech shows a speaker, its name is reset to "Speaker N" by the daily sweep.
- **PRIVACY.md and README** stop saying "not identified" and describe exactly this.

## Music (0.4.0)

Silero, the voice detector, triggers on sung vocals, so music used to be kept and its lyrics transcribed. With music rejection on (Settings › Transcription › Skip music; **on by default**):

- **The classifier ships with the app.** CED-tiny (AudioSet, Apache-2.0, `src-tauri/resources/ced-tiny-audioset.int8.onnx`, 6.1 MB) runs in the same `tuldmi-speakers` worker, as a `Classify` request. The worker loads it with `--sound` / `--sound-labels`, alongside or instead of the speaker models. No download is needed.
- **Before Whisper**, each event of either source that is at least 1 s long and not gated as quiet is scored for Speech, Music and Singing: the highest score over windows of at most 10 s.
- **The rule** (`speakers::music::is_music`): music when speech < 0.2 and the higher of music and singing ≥ 0.2.
  - Calibrated on 2026-11-10 against Wikimedia Commons music (solo piano, a cappella choir, sung anthem, radio jingle), AMI meetings and a NASA podcast.
  - Speech never scored music above 0.04. Music never scored speech above 0.06. Speech over music scored speech 0.44 or higher, so it is kept.
- **A music event** is saved `empty` with the note `MUSIC_NOTE`, like quiet clips: no transcript, and its audio is kept as usual. Under delete-after-transcription it gets the quiet clips' 24 h grace.
- **The timeline** shows it collapsed ("Music · 2 s"). The speech detector chops music into short clips, so consecutive music events become one run ("Music · 40 s · 10 clips") that opens to show each clip.
- **False positives:** **Transcribe anyway** on a clip or a run requeues it with `audio_events.transcribe_anyway = 1`, which skips the music check.
- **Any doubt transcribes as before:** the switch off, the classifier or worker missing, a worker failure, or a clip under 1 s.
- **Later (0.4.1):** choosing a different model for one section and reprocessing it.
