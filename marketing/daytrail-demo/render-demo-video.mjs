#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(__dirname, "../..");
const outputDir = resolve(repoRoot, "docs/videos");
const buildDir = resolve(__dirname, "build");
const frameDir = resolve(buildDir, "frames");
const htmlPath = resolve(__dirname, "storyboard.hyperframes.html");
const voicePath = resolve(buildDir, "daytrail-demo-voice.aiff");
const tempVideoPath = resolve(buildDir, "daytrail-demo-silent.mp4");
const outputPath = resolve(outputDir, "daytrail-demo.mp4");

const chromePath = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const ffmpegPath = "ffmpeg";

const scenes = [
  {
    duration: 6,
    type: "hook",
    eyebrow: "For knowledge workers",
    headline: "Your memory is not a timesheet.",
    body: "DayTrail shows where your workday actually went.",
  },
  {
    duration: 8,
    image: "../../docs/screenshots/01-today.png",
    eyebrow: "Today, reconstructed",
    headline: "See the day hour by hour.",
    body: "Apps, tabs, editors, terminals, and AI tools become one private timeline.",
  },
  {
    duration: 7,
    image: "../../docs/screenshots/01-today.png",
    eyebrow: "Any day, any range",
    headline: "Yesterday. Last week. This month.",
    body: "Switch from today to any date range when memory is fuzzy.",
  },
  {
    duration: 8,
    image: "../../docs/screenshots/02-ai-impact.png",
    eyebrow: "AI Impact",
    headline: "Know how much work runs through AI.",
    body: "ChatGPT, Claude, Codex, Copilot, and local tools are measured honestly.",
  },
  {
    duration: 8,
    image: "../../docs/screenshots/03-activity.png",
    eyebrow: "Source-backed sessions",
    headline: "Standups and reports from facts.",
    body: "Every session keeps the apps, projects, and evidence behind it.",
  },
  {
    duration: 8,
    image: "../../docs/screenshots/06-capture-health.png",
    eyebrow: "Trust the capture",
    headline: "If recording breaks, DayTrail tells you.",
    body: "No more discovering at 6 PM that your tracker silently stopped.",
  },
  {
    duration: 7,
    type: "trust",
    eyebrow: "Local-first",
    headline: "No timers. No surveillance.",
    body: "Metadata-first capture stays on your machine and keeps the trail useful.",
  },
  {
    duration: 7,
    type: "cta",
    eyebrow: "Try DayTrail",
    headline: "Find out where your time really goes.",
    body: "Download the latest build and let one real workday tell the truth.",
  },
];

const voiceover = `At the end of a busy day, memory lies.
You remember the big task, not the ten context switches, hidden AI work, or lost hour in a browser.
DayTrail builds a private timeline automatically from your apps, tabs, editors, terminals, and AI tools.
Open Today and see what happened, hour by hour.
Look at yesterday, last week, this month, or any custom range.
AI Impact shows which tools helped, where they showed up, and what still needs review.
Activity turns scattered windows into sessions and projects, so standups and reports come from facts, not guesses.
And if capture breaks, DayTrail tells you before you lose the day.
Local-first. No timer. No surveillance.
Just your workday, finally visible.
Try DayTrail and find out where your time really goes.`;

const brandMark = `<svg class="mark" viewBox="0 0 64 64" aria-hidden="true">
  <defs>
    <linearGradient id="markBg" x1="10" y1="8" x2="54" y2="56" gradientUnits="userSpaceOnUse">
      <stop stop-color="#13283b"/>
      <stop offset="1" stop-color="#081421"/>
    </linearGradient>
    <linearGradient id="markTrail" x1="12" y1="44" x2="53" y2="18" gradientUnits="userSpaceOnUse">
      <stop stop-color="#18d2c4"/>
      <stop offset="0.55" stop-color="#1d8cff"/>
      <stop offset="1" stop-color="#7a6dff"/>
    </linearGradient>
  </defs>
  <rect x="3" y="3" width="58" height="58" rx="17" fill="url(#markBg)" />
  <rect x="3.5" y="3.5" width="57" height="57" rx="16.5" fill="none" stroke="rgba(255,255,255,0.16)" />
  <path d="M13 43c7-15 17-19 29-12 5 3 8 2 10-7" fill="none" stroke="url(#markTrail)" stroke-width="5.8" stroke-linecap="round" />
  <path d="M32 18v15l10 7" fill="none" stroke="#f7fbff" stroke-width="4.8" stroke-linecap="round" stroke-linejoin="round" />
  <circle cx="32" cy="33" r="4.8" fill="#f7fbff" />
  <circle cx="13" cy="43" r="4.5" fill="#18d2c4" />
  <circle cx="52" cy="24" r="4.5" fill="#7a6dff" />
</svg>`;

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    stdio: "inherit",
    ...options,
  });
  if (result.status !== 0) {
    throw new Error(`${command} failed with status ${result.status}`);
  }
}

function fileUrl(path) {
  return pathToFileURL(path).href;
}

function framePath(index) {
  return join(frameDir, `frame-${String(index).padStart(2, "0")}.png`);
}

async function writeStoryboard() {
  const sceneMarkup = scenes
    .map((scene, index) => {
      const image = scene.image
        ? `<img class="product-shot" src="${scene.image}" alt="" />`
        : "";
      const chips =
        index === 2
          ? `<div class="range-chips"><span>Today</span><span>Yesterday</span><span>Last 7 days</span><span>This month</span><span>Custom range</span></div>`
          : "";
      const proof =
        index === 6
          ? `<div class="trust-grid"><span>Local data</span><span>Redacted URLs</span><span>Optional bridges</span></div>`
          : "";
      return `<section class="frame ${scene.type ?? "product"}" data-frame="${index}">
  <div class="scanline"></div>
  ${image}
  <div class="shade"></div>
  <div class="copy">
    <p class="eyebrow">${scene.eyebrow}</p>
    <h1>${scene.headline}</h1>
    <p class="body">${scene.body}</p>
    ${chips}
    ${proof}
  </div>
  <div class="brand">${brandMark}<strong>DayTrail</strong></div>
  <div class="progress"><span style="width:${((index + 1) / scenes.length) * 100}%"></span></div>
  <!-- frame:${index + 1} duration:${scene.duration * 1000} transition:fade -->
</section>`;
    })
    .join("\n");

  const metadata = scenes.map((scene, index) => ({
    frame: index + 1,
    durationMs: scene.duration * 1000,
    transition: "fade",
    sceneSummary: `${scene.eyebrow}: ${scene.headline}`,
  }));

  await writeFile(
    htmlPath,
    `<!doctype html>
<html>
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=1920,height=1080,initial-scale=1" />
  <title>DayTrail Demo HyperFrames</title>
  <style>
    :root {
      color-scheme: dark;
      --bg: #07111d;
      --panel: #0f2131;
      --ink: #f7fbff;
      --muted: #b3c1cf;
      --blue: #1d8cff;
      --cyan: #18d2c4;
      --line: rgba(159, 190, 220, 0.22);
    }
    * { box-sizing: border-box; }
    html, body { margin: 0; width: 1920px; height: 1080px; overflow: hidden; background: var(--bg); font-family: Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    .frame { position: absolute; inset: 0; display: none; overflow: hidden; background:
      radial-gradient(circle at 78% 18%, rgba(29, 140, 255, 0.22), transparent 32%),
      linear-gradient(135deg, #06101b 0%, #091827 46%, #050b12 100%); }
    .frame.active { display: block; }
    .product-shot { position: absolute; right: 86px; top: 118px; width: 1280px; max-height: 765px; object-fit: contain; border-radius: 22px; border: 1px solid var(--line); box-shadow: 0 40px 120px rgba(0, 0, 0, 0.55); filter: saturate(1.06) contrast(1.04); }
    .shade { position: absolute; inset: 0; background: linear-gradient(90deg, rgba(4, 10, 18, 0.98) 0%, rgba(4, 10, 18, 0.88) 31%, rgba(4, 10, 18, 0.25) 64%, rgba(4, 10, 18, 0.64) 100%); }
    .hook .shade, .trust .shade, .cta .shade { background: linear-gradient(135deg, rgba(5, 12, 20, 0.98), rgba(6, 20, 34, 0.94)); }
    .copy { position: absolute; z-index: 3; left: 116px; top: 146px; width: 760px; }
    .hook .copy, .trust .copy, .cta .copy { width: 1120px; top: 205px; }
    .eyebrow { margin: 0 0 28px; color: var(--cyan); text-transform: uppercase; letter-spacing: 0.14em; font-size: 28px; font-weight: 800; }
    h1 { margin: 0; color: var(--ink); font-size: 88px; line-height: 0.94; letter-spacing: 0; max-width: 1120px; text-wrap: balance; }
    .hook h1, .trust h1, .cta h1 { font-size: 118px; max-width: 1260px; }
    .body { margin: 36px 0 0; color: var(--muted); font-size: 38px; line-height: 1.28; max-width: 840px; }
    .range-chips, .trust-grid { display: flex; flex-wrap: wrap; gap: 18px; margin-top: 44px; max-width: 900px; }
    .range-chips span, .trust-grid span { border: 1px solid rgba(29, 140, 255, 0.42); background: rgba(29, 140, 255, 0.15); color: #ddecff; border-radius: 14px; padding: 18px 24px; font-size: 28px; font-weight: 750; }
    .brand { position: absolute; z-index: 4; left: 116px; bottom: 74px; display: flex; align-items: center; gap: 18px; color: var(--ink); font-size: 30px; }
    .mark { width: 60px; height: 60px; flex: 0 0 auto; filter: drop-shadow(0 14px 24px rgba(0, 0, 0, 0.32)); }
    .progress { position: absolute; z-index: 5; left: 116px; right: 116px; bottom: 42px; height: 5px; background: rgba(255, 255, 255, 0.12); border-radius: 999px; overflow: hidden; }
    .progress span { display: block; height: 100%; background: linear-gradient(90deg, var(--blue), var(--cyan)); }
    .scanline { position: absolute; inset: 0; z-index: 2; pointer-events: none; opacity: 0.09; background-image: linear-gradient(rgba(255,255,255,.11) 1px, transparent 1px); background-size: 100% 8px; }
    .hook::after, .trust::after, .cta::after { content: ""; position: absolute; right: 150px; top: 176px; width: 520px; height: 520px; border-radius: 999px; border: 1px solid rgba(29, 140, 255, .35); box-shadow: inset 0 0 70px rgba(29, 140, 255, .18), 0 0 100px rgba(24, 210, 196, .18); }
    .cta .body { color: #d9e7f5; }
  </style>
</head>
<body>
${sceneMarkup}
<script>
  const frames = [...document.querySelectorAll('.frame')];
  const params = new URLSearchParams(location.search);
  let current = Number(params.get('frame') ?? 0);
  const show = (index) => frames.forEach((frame, i) => frame.classList.toggle('active', i === index));
  show(current);
  if (!params.has('frame')) {
    setInterval(() => {
      current = (current + 1) % frames.length;
      show(current);
    }, 3000);
    addEventListener('keydown', (event) => {
      if (event.key === 'ArrowRight') current = (current + 1) % frames.length;
      if (event.key === 'ArrowLeft') current = (current - 1 + frames.length) % frames.length;
      show(current);
    });
    addEventListener('click', () => {
      current = (current + 1) % frames.length;
      show(current);
    });
  }
</script>
<!-- HYPERFRAMES_META: ${JSON.stringify(metadata)} -->
</body>
</html>
`,
  );
}

async function renderFrames() {
  if (!existsSync(chromePath)) {
    throw new Error(`Google Chrome not found at ${chromePath}`);
  }
  for (let index = 0; index < scenes.length; index += 1) {
    const url = `${fileUrl(htmlPath)}?frame=${index}`;
    run(chromePath, [
      "--headless=new",
      "--disable-gpu",
      "--hide-scrollbars",
      "--no-first-run",
      "--window-size=1920,1080",
      `--screenshot=${framePath(index)}`,
      url,
    ]);
  }
}

function buildXfadeFilter() {
  const transition = 0.55;
  const parts = scenes.map((scene, index) => {
    const fadeOutStart = Math.max(scene.duration - 0.35, 0);
    return `[${index}:v]scale=1920:1080,setsar=1,format=yuv420p,fade=t=in:st=0:d=0.25,fade=t=out:st=${fadeOutStart}:d=0.35[v${index}]`;
  });

  let previous = "v0";
  let elapsed = scenes[0].duration;
  for (let index = 1; index < scenes.length; index += 1) {
    const output = index === scenes.length - 1 ? "vout" : `x${index}`;
    const offset = Math.max(elapsed - transition, 0).toFixed(2);
    parts.push(`[${previous}][v${index}]xfade=transition=fade:duration=${transition}:offset=${offset}[${output}]`);
    previous = output;
    elapsed += scenes[index].duration - transition;
  }
  return parts.join(";");
}

async function renderSilentVideo() {
  const args = ["-y"];
  scenes.forEach((scene, index) => {
    args.push("-loop", "1", "-t", String(scene.duration), "-i", framePath(index));
  });
  args.push(
    "-filter_complex",
    buildXfadeFilter(),
    "-map",
    "[vout]",
    "-r",
    "30",
    "-c:v",
    "libx264",
    "-pix_fmt",
    "yuv420p",
    "-movflags",
    "+faststart",
    tempVideoPath,
  );
  run(ffmpegPath, args);
}

async function renderVoiceover() {
  await writeFile(resolve(__dirname, "voiceover.txt"), `${voiceover}\n`);
  const say = spawnSync("say", ["-v", "Samantha", "-r", "176", "-o", voicePath, voiceover], {
    cwd: repoRoot,
    stdio: "inherit",
  });
  return say.status === 0 && existsSync(voicePath);
}

async function muxAudio(hasVoiceover) {
  if (!hasVoiceover) {
    run("cp", [tempVideoPath, outputPath]);
    return;
  }
  run(ffmpegPath, [
    "-y",
    "-i",
    tempVideoPath,
    "-i",
    voicePath,
    "-filter_complex",
    "[1:a]volume=1.15,apad[a]",
    "-map",
    "0:v",
    "-map",
    "[a]",
    "-c:v",
    "copy",
    "-c:a",
    "aac",
    "-b:a",
    "160k",
    "-shortest",
    "-movflags",
    "+faststart",
    outputPath,
  ]);
}

async function main() {
  await mkdir(outputDir, { recursive: true });
  await mkdir(frameDir, { recursive: true });
  await writeStoryboard();
  await renderFrames();
  await renderSilentVideo();
  const hasVoiceover = await renderVoiceover();
  await muxAudio(hasVoiceover);
  console.log(`Rendered ${outputPath}`);
  console.log(`Storyboard ${htmlPath}`);
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
