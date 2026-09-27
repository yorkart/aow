import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { resolve, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { buildSnapshot } from "./build-snapshot.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export function defaultSiteUrl(repository = "yorkart/aow") {
  const [owner, name] = repository.split("/");
  if (!owner || !name) throw new Error("Expected an owner/repository name");
  return `https://${owner.toLowerCase()}.github.io/${name.toLowerCase() === `${owner.toLowerCase()}.github.io` ? "" : `${name}/`}`;
}

export async function buildSite({
  siteUrl = defaultSiteUrl(),
  outDir = join(root, "_site"),
  rebuildSnapshot = true,
} = {}) {
  const url = new URL(siteUrl);
  if (
    !["https:", "http:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.search ||
    url.hash
  ) {
    throw new Error(
      "SITE_URL must be an HTTP(S) site URL without credentials, query or fragment",
    );
  }
  url.pathname = `${url.pathname.replace(/\/+$/, "")}/`;
  const escape = (value) =>
    value
      .replaceAll("&", "&amp;")
      .replaceAll('"', "&quot;")
      .replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;");
  const replacements = {
    SITE_URL: escape(url.href),
    BASE_PATH: escape(url.pathname),
  };
  if (rebuildSnapshot) await buildSnapshot(join(root, "_snapshot"));
  await rm(outDir, { recursive: true, force: true });
  await mkdir(outDir, { recursive: true });
  for (const name of ["index.html", "404.html"]) {
    const source = await readFile(join(root, name), "utf8");
    const html = source.replace(
      /\{\{(SITE_URL|BASE_PATH)\}\}/g,
      (_, key) => replacements[key],
    );
    if (/\{\{\w+\}\}/.test(html))
      throw new Error(`Unresolved template in ${name}`);
    await writeFile(join(outDir, name), html);
  }
  for (const name of [
    "styles.css",
    "main.js",
    "snapshot-embed.css",
    "snapshot-embed.js",
    "snapshot-route.js",
    "assets",
  ]) {
    await cp(join(root, name), join(outDir, name), { recursive: true });
  }
  await cp(join(root, "_snapshot"), join(outDir, "snapshot"), {
    recursive: true,
  });
  await writeFile(join(outDir, ".nojekyll"), "");
  await writeFile(
    join(outDir, "sitemap.xml"),
    `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9"><url><loc>${escape(url.href)}</loc></url></urlset>\n`,
  );
  return { outDir, siteUrl: url.href, basePath: url.pathname };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  const result = await buildSite({
    siteUrl:
      process.env.SITE_URL || defaultSiteUrl(process.env.GITHUB_REPOSITORY),
  });
  console.log(`Built ${result.siteUrl} → ${result.outDir}`);
}
