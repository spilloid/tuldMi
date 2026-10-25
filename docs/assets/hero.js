// The hero's "tape": a simulation of what tuldMi does, with made-up speech. Two lanes (Me = microphone, Remote = what the
// computer plays) scroll right to left. Speech bursts are kept with a 300 ms pre-roll and 700 ms post-roll, exactly the app's
// defaults; silence is drawn faded because it is never stored. Each kept burst becomes a transcript line (DOM, not canvas,
// so text stays text), then a search for "thursday" lights up the matches, and the loop starts over.
// Everything runs on simulated time in fixed ticks, so pausing, hiding the tab or scrolling away freezes it exactly.
(() => {
  "use strict";

  const canvas = document.getElementById("tape");
  const lines = document.getElementById("lines");
  if (!canvas || !lines || !canvas.getContext) return;

  const ctx = canvas.getContext("2d");
  const stage = document.getElementById("stage");
  const rec = document.getElementById("rec");
  const recLabel = document.getElementById("rec-label");
  const pauseBtn = document.getElementById("tape-pause");
  const replayBtn = document.getElementById("tape-replay");
  const find = stage.querySelector(".find");
  const findQ = document.getElementById("find-q");
  const findHint = document.getElementById("find-hint");
  const findHintIdle = findHint.innerHTML;

  const TICK = 50;            // ms of simulated time per column
  const STEP = 3;             // px per column (2 px bar + 1 px gap)
  const PRE = 300 / TICK;     // pre-roll columns
  const POST = 700 / TICK;    // post-roll columns
  const QUERY = "thursday";

  // gap: silence before the line starts (ms). Clock: the transcript's wall time, as the app would show it.
  const SCRIPT = [
    { src: "remote", gap: 900, dur: 3000, clock: "14:02", text: "Quick one: the install moved to Thursday at nine thirty." },
    { src: "me", gap: 500, dur: 2300, clock: "14:02", text: "Thursday works. Same address as last time?" },
    { src: "remote", gap: 450, dur: 3300, clock: "14:03", text: "Same address. Bring the serial numbers for the two old units." },
    { quiet: "38 m quiet", gap: 2600 },
    { src: "me", gap: 0, dur: 2600, clock: "14:41", text: "Note to self: ask Dana about parking on Thursday." },
  ];

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const darkScheme = window.matchMedia("(prefers-color-scheme: dark)");

  let colors = {};
  let width = 0, height = 0, dpr = 1;
  let cols = [];              // newest last: { me, remote, keptMe, keptRemote }
  let maxCols = 0;
  let sim;                    // per-loop state, see reset()
  let userPaused = false, visible = true, raf = 0, last = 0, acc = 0;

  function readColors() {
    const s = getComputedStyle(document.documentElement);
    const v = (n) => s.getPropertyValue(n).trim();
    colors = {
      me: v("--me"), meSoft: v("--me-soft"), remote: v("--remote"), remoteSoft: v("--remote-soft"),
      faint: v("--faint"), line: v("--line"), accent: v("--accent"), canvas: v("--canvas"),
    };
  }

  function resize() {
    const r = canvas.getBoundingClientRect();
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    width = Math.max(1, Math.round(r.width));
    height = Math.max(1, Math.round(r.height));
    canvas.width = width * dpr;
    canvas.height = height * dpr;
    maxCols = Math.ceil(width / STEP) + PRE + 4;
    draw(acc / TICK);
  }

  // ---- simulation ------------------------------------------------------------------------------------------------------

  function reset(clearTape) {
    if (clearTape) cols = [];
    const sched = [];
    let t = 0;
    for (const item of SCRIPT) {
      t += item.gap;
      if (item.quiet) { sched.push({ ...item, start: t, end: t }); continue; }
      sched.push({ ...item, start: t, end: t + item.dur, phase: Math.random() * 6.28 });
      t += item.dur;
    }
    sim = { t: 0, sched, next: 0, runs: { me: null, remote: null }, timers: [], pending: 0, phase: "talk", typed: 0 };
    lines.classList.remove("fading");
    lines.replaceChildren();
    findQ.textContent = "";
    find.classList.remove("active");
    findHint.innerHTML = findHintIdle;
  }

  function at(delay, fn) { sim.timers.push({ at: sim.t + delay, fn }); }

  function speaking(src) {
    for (const s of sim.sched) if (s.src === src && sim.t >= s.start && sim.t < s.end) return s;
    return null;
  }

  // A speech-like envelope: syllable-rate wobble, short dips between words, soft attack and release.
  function amplitude(s) {
    if (!s) return 0.035 + Math.random() * 0.03;
    const k = sim.t - s.start, left = s.end - sim.t;
    const edge = Math.min(1, k / 140, left / 180);
    const syll = Math.abs(Math.sin(k / 1000 * 2 * Math.PI * 3.6 + s.phase));
    const word = Math.sin(k / 1000 * 2 * Math.PI * 0.9 + s.phase * 2) > -0.82 ? 1 : 0.18;
    return Math.max(0.05, edge * word * (0.28 + 0.62 * syll * (0.65 + 0.35 * Math.random())));
  }

  function tick() {
    sim.t += TICK;

    // Scheduled DOM work (transcription finishing, typing) runs on simulated time too.
    for (let i = sim.timers.length - 1; i >= 0; i--) {
      if (sim.timers[i].at <= sim.t) { const { fn } = sim.timers[i]; sim.timers.splice(i, 1); fn(); }
    }

    while (sim.next < sim.sched.length && sim.sched[sim.next].quiet && sim.t >= sim.sched[sim.next].start) {
      addQuiet(sim.sched[sim.next].quiet);
      sim.next++;
    }
    while (sim.next < sim.sched.length && !sim.sched[sim.next].quiet && sim.t >= sim.sched[sim.next].start) sim.next++;

    const col = { me: 0, remote: 0, keptMe: false, keptRemote: false };
    for (const src of ["me", "remote"]) {
      const s = sim.phase === "talk" ? speaking(src) : null;
      col[src] = amplitude(s);
      const key = src === "me" ? "keptMe" : "keptRemote";
      let run = sim.runs[src];
      if (s && !run) {
        // Speech starts: keep the pre-roll that is already on the tape.
        run = sim.runs[src] = { item: s, tail: 0 };
        for (let i = Math.max(0, cols.length - PRE); i < cols.length; i++) cols[i][key] = true;
      }
      if (run) {
        if (s) { col[key] = true; run.tail = 0; }
        else if (run.tail < POST) { col[key] = true; run.tail++; }
        else { sim.runs[src] = null; transcribe(run.item); }
      }
    }
    cols.push(col);
    if (cols.length > maxCols) cols.splice(0, cols.length - maxCols);

    const talkDone = sim.next >= sim.sched.length && !sim.runs.me && !sim.runs.remote && sim.pending === 0;
    if (sim.phase === "talk" && talkDone) { sim.phase = "search"; at(1100, typeNext); }
  }

  function transcribe(item) {
    sim.pending++;
    const li = document.createElement("li");
    li.className = `line ${item.src}`;
    li.dataset.text = item.text;
    const time = document.createElement("time");
    time.textContent = item.clock;
    const body = document.createElement("div");
    body.className = "body";
    const who = document.createElement("span");
    who.className = "who";
    who.textContent = item.src === "me" ? "Me" : "Remote";
    const pending = document.createElement("div");
    pending.className = "pending";
    pending.innerHTML = '<span class="bar"></span>Transcribing…';
    body.append(who, pending);
    li.append(time, body);
    append(li);
    at(700 + item.text.length * 9, () => {
      const p = document.createElement("p");
      p.textContent = item.text;
      pending.replaceWith(p);
      sim.pending--;
    });
  }

  function addQuiet(label) {
    const li = document.createElement("li");
    li.className = "quiet";
    li.textContent = label;
    append(li);
  }

  function append(li) {
    lines.append(li);
    while (lines.children.length > 5) lines.firstElementChild.remove();
  }

  function typeNext() {
    find.classList.add("active");
    sim.typed++;
    findQ.textContent = QUERY.slice(0, sim.typed);
    if (sim.typed < QUERY.length) { at(95 + Math.random() * 70, typeNext); return; }
    const hits = highlight(QUERY);
    findHint.innerHTML = `<span class="count">${hits}</span>&nbsp;matches`;
    sim.phase = "hold";
    at(5200, () => { lines.classList.add("fading"); sim.phase = "fade"; });
    at(5800, () => reset(false));
  }

  function highlight(q) {
    let hits = 0;
    for (const li of lines.querySelectorAll(".line")) {
      const p = li.querySelector("p");
      const text = li.dataset.text;
      const i = text.toLowerCase().indexOf(q);
      if (!p || i < 0) { li.classList.add("dim"); continue; }
      hits++;
      li.classList.add("hit");
      const mark = document.createElement("mark");
      mark.textContent = text.slice(i, i + q.length);
      p.replaceChildren(text.slice(0, i), mark, text.slice(i + q.length));
    }
    return hits;
  }

  // ---- drawing ---------------------------------------------------------------------------------------------------------

  function draw(frac) {
    if (!width) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const lane = height / 2;
    const half = lane / 2 - 10;
    const nowX = width - 18;
    const shift = frac * STEP;

    ctx.strokeStyle = colors.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, Math.round(lane) + 0.5);
    ctx.lineTo(width, Math.round(lane) + 0.5);
    ctx.stroke();

    [["me", "keptMe", lane / 2, colors.me, colors.meSoft], ["remote", "keptRemote", lane * 1.5, colors.remote, colors.remoteSoft]]
      .forEach(([src, key, mid, ink, soft]) => {
        const x = (i) => nowX - (cols.length - 1 - i) * STEP - shift;

        // Kept runs get a soft rounded backing, so pre-roll and post-roll read as part of the clip.
        ctx.fillStyle = soft;
        for (let i = 0; i < cols.length; i++) {
          if (!cols[i][key]) continue;
          let j = i;
          while (j + 1 < cols.length && cols[j + 1][key]) j++;
          const x0 = x(i) - 3, x1 = x(j) + 5;
          roundRect(x0, mid - half - 4, x1 - x0, half * 2 + 8, 7);
          i = j;
        }

        for (let i = 0; i < cols.length; i++) {
          const c = cols[i], cx = x(i);
          if (cx < -STEP) continue;
          const kept = c[key];
          const h = Math.max(1, c[src] * half);
          ctx.globalAlpha = kept ? 1 : 0.5;
          ctx.fillStyle = kept ? ink : colors.faint;
          if (kept) ctx.fillRect(cx, mid - h, 2, h * 2);
          else if (i % 2 === 0) ctx.fillRect(cx, mid - 0.5, 2, 1);
        }
        ctx.globalAlpha = 1;
      });

    // "Now": the live edge.
    const grad = ctx.createLinearGradient(nowX - 40, 0, nowX + 18, 0);
    grad.addColorStop(0, "transparent");
    grad.addColorStop(1, colors.canvas);
    ctx.fillStyle = grad;
    ctx.fillRect(nowX + 2, 0, width - nowX, height);
    ctx.fillStyle = colors.accent;
    ctx.fillRect(nowX + 4, 8, 1.5, height - 16);
  }

  function roundRect(x, y, w, h, r) {
    ctx.beginPath();
    if (ctx.roundRect) ctx.roundRect(x, y, w, h, r);
    else ctx.rect(x, y, w, h);
    ctx.fill();
  }

  // ---- loop and controls -----------------------------------------------------------------------------------------------

  function frame(now) {
    raf = 0;
    const dt = Math.min(250, now - (last || now));
    last = now;
    acc += dt;
    while (acc >= TICK) { tick(); acc -= TICK; }
    draw(acc / TICK);
    schedule();
  }

  function running() { return !userPaused && visible && !document.hidden && !reduceMotion.matches; }

  function schedule() {
    if (running() && !raf) raf = requestAnimationFrame(frame);
  }

  function halt() {
    if (raf) cancelAnimationFrame(raf);
    raf = 0;
    last = 0;
  }

  function sync() {
    rec.classList.toggle("paused", userPaused);
    recLabel.textContent = userPaused ? "Paused" : "Listening";
    pauseBtn.textContent = userPaused ? "Resume" : "Pause";
    pauseBtn.setAttribute("aria-pressed", String(userPaused));
    if (running()) schedule(); else halt();
  }

  // Reduced motion: one still frame of the finished story, search and all. The tape is the moment mid-conversation
  // when both lanes are on screen, not the quiet end of the loop.
  function still() {
    halt();
    reset(true);
    const snapAt = sim.sched[2].end + 400;
    let snap = null, guard = 0;
    while (sim.phase !== "hold" && guard++ < 2000) {
      tick();
      if (!snap && sim.t >= snapAt) snap = cols.slice();
    }
    if (snap) cols = snap;
    acc = 0;
    draw(0);
  }

  pauseBtn.addEventListener("click", () => { userPaused = !userPaused; sync(); });
  replayBtn.addEventListener("click", () => {
    reset(true);
    acc = 0;
    if (reduceMotion.matches) still();
    else { userPaused = false; sync(); }
  });
  document.addEventListener("visibilitychange", sync);
  reduceMotion.addEventListener?.("change", () => (reduceMotion.matches ? still() : (reset(true), sync())));
  darkScheme.addEventListener?.("change", () => { readColors(); draw(acc / TICK); });
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(([e]) => { visible = e.isIntersecting; sync(); }).observe(canvas);
  }
  if ("ResizeObserver" in window) new ResizeObserver(resize).observe(canvas);
  else window.addEventListener("resize", resize);

  readColors();
  resize();
  if (reduceMotion.matches) {
    pauseBtn.hidden = true;
    still();
  } else {
    reset(true);
    sync();
  }
})();
