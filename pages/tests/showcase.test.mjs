import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const html = await readFile(resolve(root, "site/index.html"), "utf8");
const script = await readFile(resolve(root, "site/showcase.js"), "utf8");

test("Pages showcase is explicit presentation-only mock UI", () => {
  assert.match(html, /Concept UI only/);
  assert.match(html, /does not call a backend/);
  assert.doesNotMatch(script, /\bfetch\s*\(/);
  assert.doesNotMatch(script, /XMLHttpRequest/);
});

test("timeline and chat are URL-addressable independent views", () => {
  assert.match(html, /data-view="timeline"/);
  assert.match(html, /data-view="chat"/);
  assert.match(script, /URLSearchParams\(location\.search\)/);
  assert.match(script, /history\.pushState/);
});

test("showcase keeps basic accessibility affordances", () => {
  assert.match(html, /class="skip-link"/);
  assert.match(html, /aria-live="polite"/);
  assert.match(script, /aria-current/);
  assert.match(html, /<label for="mock-message"/);
});
