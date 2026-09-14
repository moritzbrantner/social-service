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

test("follow request operations keep incoming and outgoing directions explicit", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    if (init?.method === "PUT" || init?.method === "DELETE") {
      return new Response(null, { status: 204 });
    }
    return new Response(JSON.stringify([]), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createSocialClient(options(fetch));

  await client.requestFollow("user-2");
  await client.cancelFollowRequest("user-2");
  await client.acceptFollowRequest("user-3");
  await client.declineFollowRequest("user-4");
  await client.incomingFollowRequests(25);
  await client.outgoingFollowRequests(10);

  assert.deepEqual(
    calls.map(({ input, init }) => [input, init.method]),
    [
      ["https://social.example/v1/follow-requests/outgoing/user-2", "PUT"],
      ["https://social.example/v1/follow-requests/outgoing/user-2", "DELETE"],
      ["https://social.example/v1/follow-requests/incoming/user-3/accept", "PUT"],
      ["https://social.example/v1/follow-requests/incoming/user-4", "DELETE"],
      ["https://social.example/v1/follow-requests/incoming?limit=25", undefined],
      ["https://social.example/v1/follow-requests/outgoing?limit=10", undefined],
    ],
  );
});
