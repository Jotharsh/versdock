// main.js — the line-highlight engine.
//
// Rust streams position ticks at 30 Hz; this rAF loop interpolates between
// ticks so the highlight moves at native display smoothness. The active
// line is chosen by binary search over the parsed LRC timeline, and the
// DOM is only repainted when the line actually changes (dirty-region rule
// from DESIGN.md — an always-on transparent window must not repaint
// constantly or it cooks the battery).

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const els = {
  lyrics: document.getElementById("lyrics"),
  fan: document.getElementById("fan"),
  wait: document.getElementById("wait"),
  title: document.getElementById("t-title"),
  artist: document.getElementById("t-artist"),
  offset: document.getElementById("offset"),
  flash: document.getElementById("flash"),
  bPlay: document.getElementById("b-play"),
  bEarly: document.getElementById("b-early"),
  bLate: document.getElementById("b-late"),
  bLock: document.getElementById("b-lock"),
};

let lines = []; // { t, text, el }
let activeIdx = -1;

// last extrapolation anchor (updated by every clock tick)
let anchor = { pos: 0, at: performance.now(), playing: false };

let posNow = () =>
  Math.max(0, anchor.pos + (anchor.playing ? (performance.now() - anchor.at) / 1000 : 0));

function binarySearch(pos) {
  let lo = 0,
    hi = lines.length - 1,
    idx = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= pos) {
      idx = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return idx;
}

function setActive(idx, force = false) {
  if (idx === activeIdx && !force) return;
  const prev = lines[activeIdx];
  if (!force && prev && prev.el) prev.el.classList.remove("active");
  activeIdx = idx;
  if (idx < 0 || !lines[idx]) return;
  const el = lines[idx].el;
  if (!force) el.classList.add("active");
  // re-center the fan on the active line
  const box = els.lyrics.getBoundingClientRect();
  const top = el.offsetTop;
  const h = el.offsetHeight;
  els.fan.style.transform = `translateY(${box.height / 2 - top - h / 2}px)`;
  // blur everything far from the active line (±1 visible, rest faded)
  const all = els.fan.children;
  for (let i = 0; i < all.length; i++) {
    all[i].classList.toggle("far", Math.abs(i - idx) > 1);
  }
}

function raf() {
  if (lines.length) setActive(binarySearch(posNow()));
  requestAnimationFrame(raf);
}

// keep the active line dead-center when the user resizes the overlay
window.addEventListener("resize", () => {
  if (activeIdx >= 0) setActive(activeIdx, true);
});

function buildTimeline(raw) {
  els.fan.innerHTML = "";
  lines = raw.map((l) => {
    const el = document.createElement("div");
    el.className = "line";
    el.textContent = l.text;
    els.fan.appendChild(el);
    return { t: l.t, text: l.text, el };
  });
  activeIdx = -1;
  els.wait.classList.add("gone");
  els.lyrics.classList.remove("hidden");
}

function setMeta(s) {
  els.title.textContent = s.title;
  els.artist.textContent = "· " + s.artist;
  document.body.classList.toggle("paused", !s.playing);
  const ic = document.getElementById("ic-play")?.firstElementChild;
  if (ic) ic.setAttribute("d", s.playing ? "M7 5.5h3.4v13H7zM13.6 5.5H17v13h-3.4z" : "M8 5.5v13l11-6.5z");
  // no title → nothing playing system-wide: idle screen
  const idle = !s.title;
  els.wait.classList.toggle("gone", !idle);
  els.lyrics.classList.toggle("hidden", idle || lines.length === 0);
}

function flash(msg, ms = 1600) {
  els.flash.textContent = msg;
  els.flash.classList.add("show");
  clearTimeout(flash._t);
  flash._t = setTimeout(() => els.flash.classList.remove("show"), ms);
}

async function boot() {
  const raw = await invoke("get_timeline");
  if (raw.length) buildTimeline(raw);
  else {
    // nothing loaded yet — wait for the first lyrics-timeline event
    els.wait.classList.remove("gone");
  }
  const s = await invoke("get_state");
  anchor = { pos: s.position, at: performance.now(), playing: s.playing };
  setMeta(s);
}
boot();

// ── events from the Rust clock ──────────────────────────────
await listen("playback-tick", (e) => {
  anchor = { pos: e.payload, at: performance.now(), playing: anchor.playing };
});

await listen("media-state", (e) => {
  const s = e.payload;
  anchor = { pos: s.position, at: performance.now(), playing: s.playing };
  setMeta(s);
});

// live lyric swaps (demo seed → LRCLIB fetches → track changes)
await listen("lyrics-timeline", (e) => {
  if (Array.isArray(e.payload) && e.payload.length) {
    buildTimeline(e.payload);
  }
});

await listen("lyrics-miss", () => {
  if (!lines.length) flash("No lyrics available for this track");
});

// ── toolbar ─────────────────────────────────────────────────
els.bPlay.addEventListener("click", async () => {
  const now = posNow();
  const playing = await invoke("toggle_play");
  anchor = { pos: now, at: performance.now(), playing };
  setMeta({ title: els.title.textContent, artist: els.artist.textContent.slice(2), playing });
});

async function bump(delta) {
  const off = await invoke("bump_offset", { delta });
  els.offset.textContent = (off > 0 ? "+" : "") + off.toFixed(1) + "s";
}
els.bEarly.addEventListener("click", () => bump(-0.5));
els.bLate.addEventListener("click", () => bump(0.5));

els.bLock.addEventListener("click", async () => {
  // real click-through lock now — the global hotkey ⌘⇧L unlocks
  const locked = await invoke("set_locked");
  if (locked) flash("Locked (click-through) — press ⌘⇧L to unlock", 2600);
});

// ── settings sheet ──────────────────────────────────────────
const sheet = document.getElementById("sheet");
const showSheet = (open) => {
  sheet.classList.toggle("open", open);
  document.body.classList.toggle("sheet-open", open);
};
els.bMenu = document.getElementById("b-menu");
els.bMenu.addEventListener("click", () => showSheet(!sheet.classList.contains("open")));

// backdrop glass ↔ fully clear
const glassRow = (clear) => flash(clear ? "Fully clear — no backdrop" : "Glass backdrop on");
els.bGlass.addEventListener("click", () => glassRow(document.body.classList.toggle("clear")));

// desktop pin: widget lives behind app windows, above the wallpaper
const curPin = document.getElementById("cur-pin");
async function togglePin() {
  const on = await invoke("toggle_desktop_pin");
  curPin.textContent = on ? "Desktop" : "Top";
  flash(on ? "Pinned to desktop — behind your windows" : "Back on top of windows");
}
document.getElementById("m-pin").addEventListener("click", togglePin);

// start at login
const curAuto = document.getElementById("cur-auto");
document.getElementById("m-auto").addEventListener("click", async () => {
  const on = await invoke("toggle_autostart");
  curAuto.textContent = on ? "On" : "Off";
  flash(on ? "Versdock starts at login" : "Autostart off");
});

// ── colour modes ─────────────────────────────────────────────
// "auto" = accent comes from the album artwork (Rust picks the dominant
// colour of the cover). Presets are hand-picked, chosen for legibility
// over dark glass.
const COLOR_MODES = [
  { id: "auto", label: "Album art" },
  { id: "255, 255, 255", label: "Snow white" },
  { id: "255, 196, 77", label: "Warm amber" },
  { id: "102, 212, 255", label: "Ice cyan" },
  { id: "255, 128, 220", label: "Neon rose" },
  { id: "140, 255, 160", label: "Mint green" },
];
let colorIdx = Number(localStorage.getItem("vs-color") || 0) % COLOR_MODES.length;
let lastArtAccent = null;

function applyColor() {
  const mode = COLOR_MODES[colorIdx];
  const accent = mode.id === "auto" ? (lastArtAccent || "255, 255, 255") : mode.id;
  document.body.style.setProperty("--accent", accent);
  document.getElementById("cur-color").textContent = mode.label;
}

document.getElementById("m-color").addEventListener("click", () => {
  colorIdx = (colorIdx + 1) % COLOR_MODES.length;
  localStorage.setItem("vs-color", String(colorIdx));
  applyColor();
});

// palette pushes from Rust (album-art dominant colour)
await listen("palette", (e) => {
  if (e.payload?.accent) {
    lastArtAccent = e.payload.accent;
    applyColor();
  }
});
applyColor();

// sheet rows reflect the persisted state on boot
invoke("get_pin").then((on) => {
  document.getElementById("cur-pin").textContent = on ? "Desktop" : "Top";
});
invoke("get_autostart").then((on) => {
  document.getElementById("cur-auto").textContent = on ? "On" : "Off";
});

// track changes reset the clock offset — reflect it in the label
await listen("media-state", (e) => {
  if (e.payload?.title) {
    invoke("get_offset").then((o) => {
      els.offset.textContent = (o > 0 ? "+" : "") + o.toFixed(1) + "s";
    });
  }
});

requestAnimationFrame(raf);
