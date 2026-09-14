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

test("createPost sends an explicit approved-followers audience", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    return new Response(
      JSON.stringify({
        id: "post-1",
        authorId: "user-1",
        body: "audience post",
        visibility: "private",
        audience: "approved_followers",
        createdAt: "2026-09-14T00:00:00Z",
        updatedAt: "2026-09-14T00:00:00Z",
        version: 1,
        mediaIds: [],
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  };
  const client = createSocialClient(options(fetch));

  const post = await client.createPost({
    body: "audience post",
    audience: "approved_followers",
  });

  assert.equal(calls.length, 1);
  assert.equal(calls[0].input, "https://social.example/v1/posts");
  assert.equal(calls[0].init.method, "POST");
  assert.deepEqual(JSON.parse(calls[0].init.body), {
    body: "audience post",
    audience: "approved_followers",
  });
  assert.equal(post.audience, "approved_followers");
  assert.equal(post.visibility, "private");
});
