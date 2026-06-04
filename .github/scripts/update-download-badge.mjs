#!/usr/bin/env node
import { mkdir, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";

const owner = process.env.GITHUB_REPOSITORY_OWNER || "varaprasadreddy9676";
const repo = process.env.GITHUB_REPOSITORY?.split("/")[1] || "DayTrail";
const token = process.env.GITHUB_TOKEN;
const output = resolve("docs/badges/total-downloads.svg");

function requestJson(url) {
  const headers = {
    Accept: "application/vnd.github+json",
    "User-Agent": "daytrail-download-badge",
    "X-GitHub-Api-Version": "2022-11-28",
  };
  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  return fetch(url, { headers }).then((response) => {
    if (!response.ok) {
      throw new Error(`GitHub API returned ${response.status} for ${url}`);
    }
    return response.json();
  });
}

function escapeXml(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function formatDownloads(value) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 10_000) return `${Math.round(value / 1_000)}k`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return String(value);
}

function badgeSvg(label, message) {
  const labelWidth = 150;
  const messageWidth = Math.max(48, message.length * 12 + 24);
  const width = labelWidth + messageWidth;
  const safeLabel = escapeXml(label.toUpperCase());
  const safeMessage = escapeXml(message);

  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="28" role="img" aria-label="${safeLabel}: ${safeMessage}">
  <title>${safeLabel}: ${safeMessage}</title>
  <linearGradient id="s" x2="0" y2="100%">
    <stop offset="0" stop-color="#fff" stop-opacity=".08"/>
    <stop offset="1" stop-color="#000" stop-opacity=".08"/>
  </linearGradient>
  <clipPath id="r"><rect width="${width}" height="28" rx="4" fill="#fff"/></clipPath>
  <g clip-path="url(#r)">
    <rect width="${labelWidth}" height="28" fill="#24292f"/>
    <rect x="${labelWidth}" width="${messageWidth}" height="28" fill="#2ea44f"/>
    <rect width="${width}" height="28" fill="url(#s)"/>
  </g>
  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" text-rendering="geometricPrecision" font-size="11">
    <text x="${labelWidth / 2}" y="18" fill="#fff" letter-spacing="1">${safeLabel}</text>
    <text x="${labelWidth + messageWidth / 2}" y="18" fill="#fff" font-weight="700">${safeMessage}</text>
  </g>
</svg>
`;
}

const releases = await requestJson(
  `https://api.github.com/repos/${owner}/${repo}/releases?per_page=100`,
);
const totalDownloads = releases.reduce((releaseTotal, release) => {
  const assetTotal = (release.assets || []).reduce(
    (sum, asset) => sum + Number(asset.download_count || 0),
    0,
  );
  return releaseTotal + assetTotal;
}, 0);

await mkdir(dirname(output), { recursive: true });
await writeFile(output, badgeSvg("Total Downloads", formatDownloads(totalDownloads)));
console.log(`Updated ${output} with ${totalDownloads} downloads`);
