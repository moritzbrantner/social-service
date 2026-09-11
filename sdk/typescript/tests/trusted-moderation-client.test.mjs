import assert from "node:assert/strict";
import test from "node:test";

import { createTrustedModerationClient } from "../src/trusted-moderation-client.ts";

function options(fetch, capabilities) {
  return {
    baseUrl: "https://social.example/",
    appId: "app-1",
    getUserId: async () => "adapter-1",
    getCapabilities: async () => capabilities,
    fetch,
  };
}

test("trusted moderation client ingests explainable signal evidence with least-privilege claims", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    return new Response(JSON.stringify({ id: "signal-1" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createTrustedModerationClient(options(fetch, ["signals.write"]));
  const signal = {
    targetType: "post",
    targetId: "post-1",
    source: "text-safety",
    kind: "spam",
    severity: "high",
    confidence: 0.94,
    model: "classifier",
    modelVersion: "2026-09",
    evidence: { labels: ["spam"], explanation: "repeated campaign pattern" },
    idempotencyKey: "scan-42",
  };

  assert.deepEqual(await client.ingestSignal(signal), { id: "signal-1" });
  assert.equal(calls[0].input, "https://social.example/v1/moderation/signals");
  assert.equal(calls[0].init.method, "POST");
  assert.equal(calls[0].init.body, JSON.stringify(signal));
  const headers = new Headers(calls[0].init.headers);
  assert.equal(headers.get("x-app-id"), "app-1");
  assert.equal(headers.get("x-user-id"), "adapter-1");
  assert.equal(headers.get("x-social-moderation-capabilities"), "signals.write");
  assert.equal(headers.get("x-social-moderation-capabilities")?.includes("content.moderate"), false);
});

test("trusted moderation client builds provider-neutral triage queries", async () => {
  let requestedUrl = "";
  const fetch = async (input) => {
    requestedUrl = String(input);
    return new Response("[]", {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createTrustedModerationClient(options(fetch, ["signals.read"]));

  assert.deepEqual(
    await client.signals({
      targetType: "message",
      targetId: "message-1",
      minimumSeverity: "high",
      source: "media-safety",
      limit: 20,
    }),
    [],
  );

  const url = new URL(requestedUrl);
  assert.equal(url.pathname, "/v1/moderation/signals");
  assert.equal(url.searchParams.get("targetType"), "message");
  assert.equal(url.searchParams.get("targetId"), "message-1");
  assert.equal(url.searchParams.get("minimumSeverity"), "high");
  assert.equal(url.searchParams.get("source"), "media-safety");
  assert.equal(url.searchParams.get("limit"), "20");
});
