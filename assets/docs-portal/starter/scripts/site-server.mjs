// The server the browser gate and its fixture tests read the built site from.
//
// It never closes an idle connection while a run is open. A keep-alive
// timeout lets the server close a socket at the moment a browser sends its
// next request on it, and Chromium can then wait on that request until the
// navigation times out and refuse to exit afterwards. So idle connections stay
// open until the browser closes them or the run closes the server, which ends
// every connection at once.
//
// It serves a static build the way `astro preview` does: only paths under the
// base; a route as its `index.html` or `.html` page; `404.html` with status 404
// for anything else; the content type named by the file's extension; and
// `no-cache`, so a read always gets the bytes on disk.
import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

// Node's default keep-alive timeout is 5000 ms; 0 turns the idle close off.
export const IDLE_CLOSE_DISABLED = 0;

const CONTENT_TYPES = Object.freeze({
  ".avif": "image/avif",
  ".css": "text/css",
  ".gif": "image/gif",
  ".html": "text/html; charset=utf-8",
  ".jpeg": "image/jpeg",
  ".jpg": "image/jpeg",
  ".js": "text/javascript",
  ".json": "application/json",
  ".md": "text/markdown",
  ".mjs": "text/javascript",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".txt": "text/plain",
  ".wasm": "application/wasm",
  ".webp": "image/webp",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
});

export function contentType(file) {
  return CONTENT_TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream";
}

// Serves `directory` under `base` on a loopback port the system picks.
// Returns the origin and an idempotent close that ends every connection.
export async function serveBuiltSite({ directory, base = "/" }) {
  if (typeof base !== "string" || !base.startsWith("/") || !base.endsWith("/")) throw new Error("site base must start and end with /");
  const root = path.resolve(directory);
  const server = createServer((request, response) => {
    respond(root, base, request, response).catch((error) => {
      if (!response.headersSent) send(response, request, 500, "text/plain", Buffer.from("Internal Server Error"));
      else response.destroy(error);
    });
  });
  server.keepAliveTimeout = IDLE_CLOSE_DISABLED;
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => { server.off("error", reject); resolve(); });
  });
  let closing = null;
  return {
    server,
    origin: `http://127.0.0.1:${server.address().port}`,
    close() {
      closing ??= new Promise((resolve, reject) => {
        server.close((error) => (error ? reject(error) : resolve()));
        server.closeAllConnections();
      });
      return closing;
    },
  };
}

async function respond(root, base, request, response) {
  if (request.method !== "GET" && request.method !== "HEAD") {
    response.setHeader("allow", "GET, HEAD");
    return send(response, request, 405, "text/plain", Buffer.from("Method Not Allowed"));
  }
  let pathname;
  try { pathname = decodeURIComponent(new URL(request.url, "http://127.0.0.1").pathname); }
  catch { return send(response, request, 400, "text/plain", Buffer.from("Bad Request")); }
  if (pathname.includes("\0")) return send(response, request, 400, "text/plain", Buffer.from("Bad Request"));
  if (pathname.startsWith(base)) {
    const route = path.posix.normalize(`/${pathname.slice(base.length)}`);
    const candidates = route.endsWith("/")
      ? [`${route}index.html`, ...(route === "/" ? [] : [`${route.slice(0, -1)}.html`])]
      : [route, `${route}.html`, `${route}/index.html`];
    for (const candidate of candidates) {
      const file = await regularFile(root, candidate);
      if (file) return send(response, request, 200, contentType(file), await readFile(file));
    }
  }
  const missing = await regularFile(root, "/404.html");
  if (missing) return send(response, request, 404, contentType(missing), await readFile(missing));
  return send(response, request, 404, "text/plain", Buffer.from("Not Found"));
}

async function regularFile(root, route) {
  const file = path.join(root, route);
  if (file !== root && !file.startsWith(`${root}${path.sep}`)) return null;
  try { return (await stat(file)).isFile() ? file : null; } catch { return null; }
}

function send(response, request, status, type, body) {
  response.writeHead(status, { "content-type": type, "content-length": body.length, "cache-control": "no-cache" });
  response.end(request.method === "HEAD" ? undefined : body);
}
