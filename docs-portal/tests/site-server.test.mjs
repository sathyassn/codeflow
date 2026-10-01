// The browser gate's own server. It keeps idle connections open for the whole
// run, so a browser never sends a page request on a socket the server has
// just closed, and it serves a static build the way `astro preview` does.
import assert from "node:assert/strict";
import { createServer, Agent, request as httpRequest } from "node:http";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { setTimeout as sleep } from "node:timers/promises";
import { IDLE_CLOSE_DISABLED, serveBuiltSite } from "../scripts/site-server.mjs";

// Node's own default: the idle close the verifier's server turns off.
const NODE_DEFAULT_KEEP_ALIVE_MS = 5_000;

function get(url, { agent, method = "GET" } = {}) {
  return new Promise((resolve, reject) => {
    const outgoing = httpRequest(url, { agent, method }, (response) => {
      const chunks = [];
      response.on("data", (chunk) => chunks.push(chunk));
      response.on("end", () => resolve({ status: response.statusCode, headers: response.headers, body: Buffer.concat(chunks).toString("utf8"), reused: outgoing.reusedSocket }));
      response.on("error", reject);
    });
    outgoing.on("error", reject);
    outgoing.end();
  });
}

async function builtSite() {
  const directory = await mkdtemp(path.join(os.tmpdir(), "codeflow-site-server-"));
  const dist = path.join(directory, "dist");
  await mkdir(path.join(dist, "guide"), { recursive: true });
  await mkdir(path.join(dist, "_astro"), { recursive: true });
  await writeFile(path.join(dist, "index.html"), "<p>home</p>");
  await writeFile(path.join(dist, "guide/index.html"), "<p>guide</p>");
  await writeFile(path.join(dist, "flat.html"), "<p>flat</p>");
  await writeFile(path.join(dist, "404.html"), "<p>missing</p>");
  await writeFile(path.join(dist, "_astro/page.js"), "void 0;");
  await writeFile(path.join(dist, "_astro/page.css"), "p{}");
  await writeFile(path.join(dist, "guide.md"), "# Guide");
  await writeFile(path.join(dist, "search.pf_meta"), "meta");
  await writeFile(path.join(directory, "secret.txt"), "outside the site");
  return { directory, dist };
}

test("the verifier serves through a server that never closes an idle connection", { timeout: 60_000 }, async () => {
  const { directory, dist } = await builtSite();
  const site = await serveBuiltSite({ directory: dist });
  // The control is a Node server with its default keep-alive, the setting the
  // stalled runs had: the same idle gap loses its socket there.
  const control = createServer((request, response) => response.end("control"));
  await new Promise((resolve) => control.listen(0, "127.0.0.1", resolve));
  const siteAgent = new Agent({ keepAlive: true, maxSockets: 1 });
  const controlAgent = new Agent({ keepAlive: true, maxSockets: 1 });
  try {
    assert.equal(IDLE_CLOSE_DISABLED, 0);
    assert.equal(site.server.keepAliveTimeout, IDLE_CLOSE_DISABLED);
    assert.equal(control.keepAliveTimeout, NODE_DEFAULT_KEEP_ALIVE_MS);
    assert.equal(site.server.timeout, 0);
    assert.equal(site.server.maxRequestsPerSocket, 0);
    const controlUrl = `http://127.0.0.1:${control.address().port}/`;
    assert.equal((await get(`${site.origin}/`, { agent: siteAgent })).reused, false);
    assert.equal((await get(controlUrl, { agent: controlAgent })).reused, false);
    await sleep(NODE_DEFAULT_KEEP_ALIVE_MS + 1_000);
    const again = await get(`${site.origin}/guide/`, { agent: siteAgent });
    assert.deepEqual([again.status, again.body, again.reused], [200, "<p>guide</p>", true], "the idle socket is still open and serves the next page");
    assert.equal((await get(controlUrl, { agent: controlAgent })).reused, false, "the default keep-alive closed its idle socket");
  } finally {
    siteAgent.destroy();
    controlAgent.destroy();
    await new Promise((resolve) => control.close(resolve));
    await site.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("the verifier's server serves a static build the way the preview does", async () => {
  const { directory, dist } = await builtSite();
  const site = await serveBuiltSite({ directory: dist, base: "/docs/" });
  try {
    const read = async (route, method) => {
      const response = await get(`${site.origin}${route}`, { method });
      return [response.status, response.headers["content-type"], response.body];
    };
    const html = "text/html; charset=utf-8";
    assert.deepEqual(await read("/docs/"), [200, html, "<p>home</p>"]);
    assert.deepEqual(await read("/docs/guide/"), [200, html, "<p>guide</p>"]);
    assert.deepEqual(await read("/docs/guide"), [200, html, "<p>guide</p>"]);
    assert.deepEqual(await read("/docs/guide/index.html"), [200, html, "<p>guide</p>"]);
    assert.deepEqual(await read("/docs/flat"), [200, html, "<p>flat</p>"]);
    assert.deepEqual(await read("/docs/flat/"), [200, html, "<p>flat</p>"]);
    assert.deepEqual(await read("/docs/_astro/page.js"), [200, "text/javascript", "void 0;"]);
    assert.deepEqual(await read("/docs/_astro/page.css?v=1"), [200, "text/css", "p{}"]);
    assert.deepEqual(await read("/docs/guide.md"), [200, "text/markdown", "# Guide"]);
    assert.deepEqual(await read("/docs/search.pf_meta"), [200, "application/octet-stream", "meta"]);
    // Anything else is the site's 404 page with status 404: a missing route,
    // a path outside the base, and a path that climbs out of the site.
    for (const route of ["/docs/missing/", "/docs/_astro/", "/guide/", "/docs", "/docs/%2e%2e/secret.txt", "/docs/../secret.txt"]) {
      assert.deepEqual(await read(route), [404, html, "<p>missing</p>"], route);
    }
    assert.deepEqual(await read("/docs/%E0%A4%A"), [400, "text/plain", "Bad Request"]);
    assert.deepEqual(await read("/docs/", "POST"), [405, "text/plain", "Method Not Allowed"]);
    const head = await get(`${site.origin}/docs/guide/`, { method: "HEAD" });
    assert.deepEqual([head.status, head.headers["content-length"], head.headers["cache-control"], head.body], [200, "12", "no-cache", ""]);
    await rm(path.join(dist, "404.html"));
    assert.deepEqual(await read("/docs/missing/"), [404, "text/plain", "Not Found"]);
  } finally {
    await site.close();
    await rm(directory, { recursive: true, force: true });
  }
});

test("closing the verifier's server ends its open connections", async () => {
  const { directory, dist } = await builtSite();
  const site = await serveBuiltSite({ directory: dist });
  const agent = new Agent({ keepAlive: true, maxSockets: 1 });
  try {
    await get(`${site.origin}/`, { agent });
    const [socket] = Object.values(agent.freeSockets).flat();
    assert.ok(socket, "the agent holds the idle socket");
    const ended = new Promise((resolve) => socket.once("close", resolve));
    await Promise.all([site.close(), site.close()]);
    await ended;
    await assert.rejects(get(`${site.origin}/`), { code: "ECONNREFUSED" });
    await assert.rejects(serveBuiltSite({ directory: dist, base: "docs" }), /site base must start and end with \//);
  } finally {
    agent.destroy();
    await rm(directory, { recursive: true, force: true });
  }
});
