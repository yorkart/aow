import assert from "node:assert/strict";
import { access, mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { buildSite, defaultSiteUrl } from "../scripts/build.mjs";

for (const siteUrl of [
  "https://yorkart.github.io/aow/",
  "https://someone.github.io/another-project/",
  "https://aow.example.com/",
]) {
  test(`all local links and assets work at ${siteUrl}, including a nested 404`, async (t) => {
    const temporary = await mkdtemp(join(tmpdir(), "aow-pages-"));
    t.after(() => rm(temporary, { recursive: true, force: true }));
    const output = join(temporary, "public");
    const { basePath } = await buildSite({
      siteUrl,
      outDir: output,
      rebuildSnapshot: false,
    });
    for (const file of ["index.html", "404.html"]) {
      const html = await readFile(join(output, file), "utf8");
      const ids = new Set(
        [...html.matchAll(/\bid="([^"]+)"/g)].map((match) => match[1]),
      );
      const pageUrl = new URL(
        file === "404.html" ? "missing/nested/page" : "",
        siteUrl,
      );
      for (const [, attribute, value] of html.matchAll(
        /\b(href|src)="([^"]+)"/g,
      )) {
        if (value.startsWith("#")) {
          assert.ok(ids.has(value.slice(1)), `Missing fragment ${value}`);
          continue;
        }
        const link = new URL(value, pageUrl);
        if (link.origin !== pageUrl.origin) continue;
        assert.ok(
          link.pathname.startsWith(basePath),
          `Escaped the Pages prefix: ${attribute}=${value}`,
        );
        const relative = decodeURIComponent(
          link.pathname.slice(basePath.length),
        );
        await access(join(output, relative || "index.html"));
      }
      assert.doesNotMatch(html, /\{\{\w+\}\}/);
    }
    assert.match(
      await readFile(join(output, "sitemap.xml"), "utf8"),
      new RegExp(siteUrl.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")),
    );
    assert.deepEqual((await readdir(output)).sort(), [
      ".nojekyll",
      "404.html",
      "assets",
      "index.html",
      "main.js",
      "sitemap.xml",
      "snapshot",
      "snapshot-embed.css",
      "snapshot-embed.js",
      "snapshot-route.js",
      "styles.css",
    ]);
    // Module imports must resolve alongside the entry point at every Pages prefix.
    for (const name of ["main.js", "snapshot-embed.js"]) {
      const source = await readFile(join(output, name), "utf8");
      for (const [, relative] of source.matchAll(
        /(?:from\s*|import\s*|new URL\()"(\.\/[^"\n]+)"/g,
      )) {
        const link = new URL(relative, new URL(name, siteUrl));
        assert.ok(link.pathname.startsWith(basePath));
        await access(join(output, link.pathname.slice(basePath.length)));
      }
    }
    const snapshotUrl = new URL("snapshot/", siteUrl);
    const snapshotHtml = await readFile(
      join(output, "snapshot/index.html"),
      "utf8",
    );
    for (const [, value] of snapshotHtml.matchAll(
      /\b(?:src|href)="([^"]+)"/g,
    )) {
      const asset = new URL(value, snapshotUrl);
      assert.equal(asset.origin, snapshotUrl.origin);
      assert.ok(asset.pathname.startsWith(basePath + "snapshot/"));
      await access(join(output, asset.pathname.slice(basePath.length)));
    }
    await access(join(output, "snapshot/data.json"));
    await access(join(output, "snapshot/api/fs/raw/workspace/aow/README.md"));
    const snapshotAssets = await readdir(join(output, "snapshot/assets"));
    assert.ok(
      snapshotAssets.some((name) => /editor\.worker.*\.js$/.test(name)),
      "Monaco workers must be bundled locally",
    );
  });
}

test("default URLs support project Pages, user Pages and forks", () => {
  assert.equal(defaultSiteUrl(), "https://yorkart.github.io/aow/");
  assert.equal(
    defaultSiteUrl("NewOwner/fork"),
    "https://newowner.github.io/fork/",
  );
  assert.equal(
    defaultSiteUrl("NewOwner/NewOwner.github.io"),
    "https://newowner.github.io/",
  );
});

test("invalid deployment URLs are rejected before the output is touched", async () => {
  for (const siteUrl of [
    "file:///tmp/site",
    "https://example.com/?bad=1",
    "https://user:password@example.com/",
  ]) {
    await assert.rejects(buildSite({ siteUrl }), /SITE_URL/);
  }
});
