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

test("vote operations keep votes separate from reactions", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    if (init?.method === "PUT" || init?.method === "DELETE") {
      return new Response(null, { status: 204 });
    }
    return new Response(
      JSON.stringify({
        upvotes: 2,
        downvotes: 1,
        score: 1,
        currentUserVote: "up",
      }),
      {
        status: 200,
        headers: { "content-type": "application/json" },
      },
    );
  };
  const client = createSocialClient(options(fetch));

  await client.votes("post", "post-1");
  await client.vote("comment", "comment-1", "down");
  await client.unvote("comment", "comment-1");

  assert.equal(calls[0].input, "https://social.example/v1/votes/post/post-1");
  assert.equal(calls[0].init.method, undefined);
  assert.equal(
    calls[1].input,
    "https://social.example/v1/votes/comment/comment-1/down",
  );
  assert.equal(calls[1].init.method, "PUT");
  assert.equal(calls[2].input, "https://social.example/v1/votes/comment/comment-1");
  assert.equal(calls[2].init.method, "DELETE");
});
