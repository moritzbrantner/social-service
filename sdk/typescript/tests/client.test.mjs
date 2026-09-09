import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import ts from "typescript";

const source = await readFile(new URL("../src/client.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ES2022,
    target: ts.ScriptTarget.ES2022,
  },
  fileName: "client.ts",
});
const moduleUrl = `data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`;
const { createSocialClient } = await import(moduleUrl);

function options(fetch) {
  return {
    baseUrl: "https://social.example/",
    appId: "app-1",
    getUserId: async () => "user-1",
    fetch,
  };
}

test("client composes authenticated JSON requests at the social-service boundary", async () => {
  const calls = [];
  const fetch = async (input, init) => {
    calls.push({ input: String(input), init });
    return new Response(JSON.stringify({ id: "post-1" }), {
      status: 200,
      headers: { "content-type": "application/json" },
    });
  };
  const client = createSocialClient(options(fetch));

  const result = await client.createPost({ body: "hello", visibility: "private" });

  assert.deepEqual(result, { id: "post-1" });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].input, "https://social.example/v1/posts");
  assert.equal(calls[0].init.method, "POST");
  assert.equal(calls[0].init.body, JSON.stringify({ body: "hello", visibility: "private" }));
  const headers = new Headers(calls[0].init.headers);
  assert.equal(headers.get("content-type"), "application/json");
  assert.equal(headers.get("x-app-id"), "app-1");
  assert.equal(headers.get("x-user-id"), "user-1");
});

test("client preserves successful no-content operations", async () => {
  const fetch = async () => new Response(null, { status: 204 });
  const client = createSocialClient(options(fetch));

  assert.equal(await client.deletePost("post-1"), undefined);
});

test("client exposes HTTP failure status and body without hiding service errors", async () => {
  const fetch = async () => new Response("not visible", { status: 404 });
  const client = createSocialClient(options(fetch));

  await assert.rejects(() => client.post("post-1"), /social-service 404: not visible/);
});
