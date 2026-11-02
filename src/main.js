// main.js — Versdock 2.0 Synced Lyrics Engine

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const els = {
  dock: document.getElementById("dock"),
  aura: document.getElementById("aura"),
  nowPlaying: document.getElementById("now-playing"),
  wave: document.getElementById("wave"),
  title: document.getElementById("t-title"),
  artist: document.getElementById("t-artist"),
  lyricsContainer: document.getElementById("lyrics-container"),
  fan: document.getElementById("fan"),
  idleView: document.getElementById("idle-view"),
  waitMsg: document.getElementById("wait-msg"),
  progressBar: document.getElementById("progress-bar"),
  progressFill: document.getElementById("progress-fill"),
  offset: document.getElementById("offset"),
  toast: document.getElementById("toast"),
  sheet: document.getElementById("sheet"),
  bPlay: document.getElementById("b-play"),
  bEarly: document.getElementById("b-early"),
  bLate: document.getElementById("b-late"),
  bLock: document.getElementById("b-lock"),
  bMenu: document.getElementById("b-menu"),
  bDemo: document.getElementById("b-demo"),
  curBackdrop: document.getElementById("cur-backdrop"),
  curColor: document.getElementById("cur-color"),
  curPin: document.getElementById("cur-pin"),
  curAuto: document.getElementById("cur-auto"),
  mBackdrop: document.getElementById("m-backdrop"),
  mColor: document.getElementById("m-color"),
  mPin: document.getElementById("m-pin"),
  mAuto: document.getElementById("m-auto"),
};

let lines = []; // { t, text, el }
let activeIdx = -1;
let currentTrack = { title: "", artist: "", duration: 0 };
let anchor = { pos: 0, at: performance.now(), playing: false };

const posNow = () =>
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

  // Re-center fan with physics ease
  const box = els.lyricsContainer.getBoundingClientRect();
  const top = el.offsetTop;
  const h = el.offsetHeight;
  els.fan.style.transform = `translateY(${box.height / 2 - top - h / 2}px)`;

  // Depth styling for surrounding lines
  const all = els.fan.children;
  for (let i = 0; i < all.length; i++) {
    const dist = Math.abs(i - idx);
    all[i].classList.toggle("near", dist === 1);
    all[i].classList.toggle("far", dist > 1);
  }
}

function updateProgress() {
  const currentPos = posNow();
  if (currentTrack.duration > 0) {
    const pct = Math.min(100, Math.max(0, (currentPos / currentTrack.duration) * 100));
    els.progressFill.style.width = pct + "%";
  } else {
    els.progressFill.style.width = "0%";
  }
}

function raf() {
  const currentPos = posNow();
  if (lines.length) {
    setActive(binarySearch(currentPos));
  }
  updateProgress();
  requestAnimationFrame(raf);
}

window.addEventListener("resize", () => {
  if (activeIdx >= 0) setActive(activeIdx, true);
});

function buildTimeline(raw) {
  els.fan.innerHTML = "";
  lines = raw.map((l, index) => {
    const el = document.createElement("div");
    el.className = "line";
    el.textContent = l.text || "♪";
    // Click any line to seek directly to its timestamp
    el.addEventListener("click", async (e) => {
      e.stopPropagation();
      await invoke("seek", { position: l.t });
      toast(`Seeked to ${formatTime(l.t)}`);
    });
    els.fan.appendChild(el);
    return { t: l.t, text: l.text, el };
  });
  activeIdx = -1;
  els.idleView.classList.add("gone");
  els.lyricsContainer.classList.remove("hidden");
  if (activeIdx >= 0) setActive(activeIdx, true);
}

function formatTime(sec) {
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s < 10 ? "0" : ""}${s}`;
}

function setMeta(s) {
  currentTrack = {
    title: s.title || "",
    artist: s.artist || "",
    duration: s.duration || 0,
  };

  const hasTitle = Boolean(s.title && s.title.trim().length > 0);
  els.title.textContent = hasTitle ? s.title : "Ready for music";
  els.artist.textContent = hasTitle ? (s.artist ? s.artist : "Unknown Artist") : "Versdock";

  document.body.classList.toggle("paused", !s.playing);
  els.wave.classList.toggle("paused", !s.playing);

  const ic = document.getElementById("ic-play")?.firstElementChild;
  if (ic) {
    ic.setAttribute(
      "d",
      s.playing
        ? "M6 5h4v14H6zm8 0h4v14h-4z"
        : "M8 5.5v13l11-6.5z"
    );
  }

  const idle = !hasTitle;
  els.idleView.classList.toggle("gone", !idle);
  els.lyricsContainer.classList.toggle("hidden", idle || lines.length === 0);
}

function toast(msg, ms = 2200) {
  els.toast.textContent = msg;
  els.toast.classList.add("show");
  clearTimeout(toast._t);
  toast._t = setTimeout(() => els.toast.classList.remove("show"), ms);
}

// ── Boot Sequence ──────────────────────────────────────────
async function boot() {
  try {
    const raw = await invoke("get_timeline");
    if (raw && raw.length) buildTimeline(raw);

    const s = await invoke("get_state");
    anchor = { pos: s.position, at: performance.now(), playing: s.playing };
    setMeta(s);

    const o = await invoke("get_offset");
    els.offset.textContent = (o > 0 ? "+" : "") + o.toFixed(1) + "s";
  } catch (err) {
    console.error("Boot error:", err);
  }
}
boot();

// ── Listeners from Rust ────────────────────────────────────
await listen("playback-tick", (e) => {
  anchor = { pos: e.payload, at: performance.now(), playing: anchor.playing };
});

let lastMediaKey = "";
await listen("media-state", (e) => {
  const s = e.payload;
  anchor = { pos: s.position, at: performance.now(), playing: s.playing };
  setMeta(s);

  const key = s.title ? `${s.title}|${s.artist}` : "";
  if (key && key !== lastMediaKey) {
    lastMediaKey = key;
    if (lines.length) {
      els.fan.innerHTML = "";
      lines = [];
      activeIdx = -1;
      els.lyricsContainer.classList.add("hidden");
    }
    els.waitMsg.textContent = "Fetching lyrics from LRCLIB…";
    els.idleView.classList.remove("gone");
  }

  if (s.title) {
    invoke("get_offset").then((o) => {
      els.offset.textContent = (o > 0 ? "+" : "") + o.toFixed(1) + "s";
    });
  }
});

await listen("lyrics-timeline", (e) => {
  if (Array.isArray(e.payload) && e.payload.length) {
    buildTimeline(e.payload);
  }
});

await listen("lyrics-miss", () => {
  if (!lines.length) {
    els.waitMsg.textContent = "No synced lyrics available on LRCLIB for this song.";
    els.idleView.classList.remove("gone");
  }
});

// ── Controls: Toolbar & Gestures ───────────────────────────
els.bPlay.addEventListener("click", async () => {
  const now = posNow();
  const playing = await invoke("toggle_play");
  anchor = { pos: now, at: performance.now(), playing };
  setMeta({
    title: els.title.textContent,
    artist: els.artist.textContent,
    duration: currentTrack.duration,
    playing,
  });
});

async function bump(delta) {
  const off = await invoke("bump_offset", { delta });
  els.offset.textContent = (off > 0 ? "+" : "") + off.toFixed(1) + "s";
  toast(`Sync offset: ${off > 0 ? "+" : ""}${off.toFixed(1)}s`);
}
els.bEarly.addEventListener("click", () => bump(-0.5));
els.bLate.addEventListener("click", () => bump(0.5));

els.bLock.addEventListener("click", async () => {
  const locked = await invoke("set_locked");
  toast(locked ? "Locked (click-through) • ⌘⇧L to unlock" : "Unlocked", 2800);
});

// Demo button in idle view
els.bDemo.addEventListener("click", async () => {
  await invoke("trigger_demo");
  toast("Loaded demo track", 1800);
});

// ── Settings Sheet ─────────────────────────────────────────
els.bMenu.addEventListener("click", (e) => {
  e.stopPropagation();
  els.sheet.classList.toggle("closed");
});

document.addEventListener("click", (e) => {
  if (!els.sheet.contains(e.target) && e.target !== els.bMenu) {
    els.sheet.classList.add("closed");
  }
});

// Backdrop Mode Switcher
const BACKDROP_MODES = [
  { id: "mode-glass", label: "Glass" },
  { id: "mode-clear", label: "Clear" },
  { id: "mode-oled", label: "OLED Dark" },
];
let backdropIdx = Number(localStorage.getItem("vs-backdrop") || 0) % BACKDROP_MODES.length;

function applyBackdrop() {
  const mode = BACKDROP_MODES[backdropIdx];
  document.body.classList.remove("mode-glass", "mode-clear", "mode-oled");
  document.body.classList.add(mode.id);
  els.curBackdrop.textContent = mode.label;
}
els.mBackdrop.addEventListener("click", () => {
  backdropIdx = (backdropIdx + 1) % BACKDROP_MODES.length;
  localStorage.setItem("vs-backdrop", String(backdropIdx));
  applyBackdrop();
  toast(`Backdrop: ${BACKDROP_MODES[backdropIdx].label}`);
});
applyBackdrop();

// Color Themes
const COLOR_THEMES = [
  { id: "auto", label: "Album Art", hex: "#ff5a1f", rgb: "255, 90, 31" },
  { id: "electric-cyan", label: "Electric Cyan", hex: "#00e5ff", rgb: "0, 229, 255" },
  { id: "sunset-violet", label: "Sunset Violet", hex: "#c084fc", rgb: "192, 132, 252" },
  { id: "neon-rose", label: "Neon Rose", hex: "#ff2d75", rgb: "255, 45, 117" },
  { id: "cyber-lime", label: "Cyber Lime", hex: "#10b981", rgb: "16, 185, 129" },
  { id: "solar-amber", label: "Solar Amber", hex: "#f59e0b", rgb: "245, 158, 11" },
  { id: "pure-snow", label: "Pure Snow", hex: "#ffffff", rgb: "255, 255, 255" },
];
let colorIdx = Number(localStorage.getItem("vs-theme") || 0) % COLOR_THEMES.length;
let lastArtAccent = null;

function applyColor() {
  const theme = COLOR_THEMES[colorIdx];
  const accentRgb = theme.id === "auto" ? (lastArtAccent || "255, 90, 31") : theme.rgb;
  const accentHex = theme.id === "auto" ? `rgb(${accentRgb})` : theme.hex;

  document.documentElement.style.setProperty("--accent", accentHex);
  document.documentElement.style.setProperty("--accent-rgb", accentRgb);
  els.curColor.textContent = theme.label;
}

els.mColor.addEventListener("click", () => {
  colorIdx = (colorIdx + 1) % COLOR_THEMES.length;
  localStorage.setItem("vs-theme", String(colorIdx));
  applyColor();
  toast(`Theme: ${COLOR_THEMES[colorIdx].label}`);
});

await listen("palette", (e) => {
  if (e.payload?.accent) {
    lastArtAccent = e.payload.accent;
    applyColor();
  }
});
applyColor();

// Pin to Desktop
async function togglePin() {
  const on = await invoke("toggle_desktop_pin");
  els.curPin.textContent = on ? "Desktop" : "Top";
  toast(on ? "Pinned to Desktop (behind windows)" : "Always on Top");
}
els.mPin.addEventListener("click", togglePin);
invoke("get_pin").then((on) => {
  els.curPin.textContent = on ? "Desktop" : "Top";
});

// Start at Login
els.mAuto.addEventListener("click", async () => {
  const on = await invoke("toggle_autostart");
  els.curAuto.textContent = on ? "On" : "Off";
  toast(on ? "Launch at login enabled" : "Launch at login disabled");
});
invoke("get_autostart").then((on) => {
  els.curAuto.textContent = on ? "On" : "Off";
});

requestAnimationFrame(raf);
