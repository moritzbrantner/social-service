import assert from "node:assert/strict";
import test from "node:test";

import { createSocialClient } from "../src/client.ts";

function clientWith(fetch) {
  return createSocialClient({
    baseUrl: "https://social.example/",
    appId: "app-1",
    getUserId: async () => "user-1",
    fetch,
  });
}

test("block and mute commands use private idempotent relationship routes", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    return new Response(null, { status: 204 });
  };
  const client = clientWith(fetch);

  await client.block("user-2");
  await client.unblock("user-2");
  await client.mute("user-3");
  await client.unmute("user-3");

  assert.deepEqual(
    calls.map(({ input, init }) => [new URL(input).pathname, init.method]),
    [
      ["/v1/blocks/user-2", "PUT"],
      ["/v1/blocks/user-2", "DELETE"],
      ["/v1/mutes/user-3", "PUT"],
      ["/v1/mutes/user-3", "DELETE"],
    ],
  );
  for (const { init } of calls) {
    const headers = new Headers(init.headers);
    assert.equal(headers.get("x-app-id"), "app-1");
    assert.equal(headers.get("x-user-id"), "user-1");
  }
});

test("block and mute lists remain scoped to the current client identity", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    return new Response(JSON.stringify([{ userId: "user-2", createdAt: "2026-09-11T00:00:00Z" }]), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = clientWith(fetch);

  assert.equal((await client.blocks(20))[0].userId, "user-2");
  assert.equal((await client.mutes(10))[0].userId, "user-2");

  assert.equal(new URL(calls[0].input).pathname, "/v1/blocks");
  assert.equal(new URL(calls[0].input).searchParams.get("limit"), "20");
  assert.equal(new URL(calls[1].input).pathname, "/v1/mutes");
  assert.equal(new URL(calls[1].input).searchParams.get("limit"), "10");
});
