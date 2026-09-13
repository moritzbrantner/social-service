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

test("threaded comment operations preserve the bounded HTTP contract", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    if (init?.method === "DELETE") {
      return new Response(null, { status: 204 });
    }
    return new Response("[]", {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createSocialClient(options(fetch));

  await client.commentReplies("post-1", "comment-1", 25);
  await client.replyToComment("post-1", "comment-1", "nested reply");
  await client.deleteComment("post-1", "comment-1");

  assert.equal(
    calls[0].input,
    "https://social.example/v1/posts/post-1/comments/comment-1/replies?limit=25",
  );
  assert.equal(calls[0].init.method, undefined);

  assert.equal(
    calls[1].input,
    "https://social.example/v1/posts/post-1/comments/comment-1/replies",
  );
  assert.equal(calls[1].init.method, "POST");
  assert.equal(calls[1].init.body, JSON.stringify({ body: "nested reply" }));

  assert.equal(
    calls[2].input,
    "https://social.example/v1/posts/post-1/comments/comment-1",
  );
  assert.equal(calls[2].init.method, "DELETE");
});
