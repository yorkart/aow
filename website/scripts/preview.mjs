import { createServer } from "node:http";
import { readFile, stat } from "node:fs/promises";
import { dirname, extname, join, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { buildSite, defaultSiteUrl } from "./build.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const { basePath } = await buildSite({
  siteUrl: process.env.SITE_URL || defaultSiteUrl(),
});
const directory = join(root, "_site");
const mime = {
  ".html": "text/html; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".ttf": "font/ttf",
  ".woff": "font/woff",
  ".woff2": "font/woff2",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".xml": "application/xml",
};
const server = createServer(async (request, response) => {
  const pathname = new URL(request.url, "http://localhost").pathname;
  if (basePath !== "/" && pathname === basePath.slice(0, -1)) {
    response.writeHead(301, { Location: basePath });
    response.end();
    return;
  }
  try {
    if (!pathname.startsWith(basePath)) throw new Error("Outside the site");
    const relative = decodeURIComponent(pathname.slice(basePath.length));
    let target = resolve(directory, relative || "index.html");
    if (!target.startsWith(directory + sep))
      throw new Error("Outside the output directory");
    if ((await stat(target)).isDirectory()) target = join(target, "index.html");
    const body = await readFile(target);
    response.writeHead(200, {
      "Content-Type": mime[extname(target)] || "application/octet-stream",
      "Cache-Control": "no-store",
    });
    response.end(request.method === "HEAD" ? undefined : body);
  } catch {
    response.writeHead(404, { "Content-Type": "text/html; charset=utf-8" });
    response.end(
      request.method === "HEAD"
        ? undefined
        : await readFile(join(directory, "404.html")),
    );
  }
});
server.listen(Number(process.env.PORT || 4173), "127.0.0.1", () => {
  console.log(
    `AoW portal: http://127.0.0.1:${server.address().port}${basePath}`,
  );
});
