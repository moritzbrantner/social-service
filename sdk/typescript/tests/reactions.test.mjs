import assert from "node:assert/strict";
import test from "node:test";

import { createSocialClient } from "../src/client.ts";

function options(fetch) {
  return {
    baseUrl: "https://social.example/",
    appId: "app-1",
    getUserId: () => "user-1",
    fetch,
  };
}

test("reaction operations use normalized target and type paths", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    if (init?.method === "PUT" || init?.method === "DELETE") {
      return new Response(null, { status: 204 });
    }
    return new Response(JSON.stringify({ counts: [], currentUserReactions: [] }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createSocialClient(options(fetch));

  await client.reactions("post", "post-1");
  await client.react("comment", "comment-1");
  await client.unreact("comment", "comment-1", "like");

  assert.equal(calls[0].input, "https://social.example/v1/reactions/post/post-1");
  assert.equal(calls[0].init.method, undefined);
  assert.equal(
    calls[1].input,
    "https://social.example/v1/reactions/comment/comment-1/like",
  );
  assert.equal(calls[1].init.method, "PUT");
  assert.equal(
    calls[2].input,
    "https://social.example/v1/reactions/comment/comment-1/like",
  );
  assert.equal(calls[2].init.method, "DELETE");
});
