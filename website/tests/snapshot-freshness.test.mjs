import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp, mkdir, writeFile, rm, chmod, symlink, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { checkSnapshotFreshness } from "../scripts/check-snapshot-freshness.mjs";
import { captureSourceState, blobHash } from "../scripts/snapshot-source.mjs";

test("freshness tracks product content, including uncommitted changes, without an artifact commit loop", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "aow-freshness-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const git = (...args) =>
    execFileSync("git", args, {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
    }).trim();
  git("init", "--quiet");
  git("config", "user.name", "Snapshot test");
  git("config", "user.email", "snapshot@example.invalid");
  git("config", "commit.gpgsign", "false");
  git("config", "core.hooksPath", "/dev/null");
  await mkdir(join(root, "frontend/src"), { recursive: true });
  await mkdir(join(root, "website/snapshot"), { recursive: true });
  await writeFile(join(root, "frontend/src/app.ts"), "original\n");
  await writeFile(join(root, ".gitignore"), "frontend/node_modules/\n");
  git("add", ".");
  git("commit", "--quiet", "-m", "Product source");
  const revision = git("rev-parse", "HEAD");
  await writeFile(
    join(root, "website/snapshot/data.json"),
    JSON.stringify({ revision, sourceState: captureSourceState(root, revision) }),
  );
  git("add", ".");
  git("commit", "--quiet", "-m", "Capture snapshot");
  assert.notEqual(git("rev-parse", "HEAD"), revision);
  assert.equal((await checkSnapshotFreshness(root)).fresh, true);

  await writeFile(
    join(root, "website/README.md"),
    "Portal documentation changed\n",
  );
  await mkdir(join(root, "frontend/node_modules"));
  await writeFile(
    join(root, "frontend/node_modules/runtime.js"),
    "ignored dependency\n",
  );
  assert.equal((await checkSnapshotFreshness(root)).fresh, true);

  await writeFile(join(root, "frontend/src/app.ts"), "uncommitted redesign\n");
  assert.deepEqual((await checkSnapshotFreshness(root)).changed, [
    "frontend/src/app.ts",
  ]);
  git("add", "frontend/src/app.ts");
  assert.equal((await checkSnapshotFreshness(root)).fresh, false);
  git("commit", "--quiet", "-m", "Redesign frontend");
  assert.equal((await checkSnapshotFreshness(root)).fresh, false);

  await writeFile(join(root, "frontend/src/app.ts"), "original\n");
  await writeFile(
    join(root, "frontend/src/new-panel.ts"),
    "new untracked component\n",
  );
  assert.deepEqual((await checkSnapshotFreshness(root)).changed, [
    "frontend/src/new-panel.ts",
  ]);
  await rm(join(root, "frontend/src/new-panel.ts"));
  await rm(join(root, "frontend/src/app.ts"));
  assert.deepEqual((await checkSnapshotFreshness(root)).changed, [
    "frontend/src/app.ts",
  ]);

  await writeFile(
    join(root, "website/snapshot/data.json"),
    JSON.stringify({ revision: "a".repeat(40) }),
  );
  await assert.rejects(checkSnapshotFreshness(root), /manifest is missing/);
});

test("source checks survive a new Git history and detect content, mode and symlink changes", async (t) => {
  const original = await mkdtemp(join(tmpdir(), "aow-source-"));
  const rewritten = await mkdtemp(join(tmpdir(), "aow-rewritten-"));
  t.after(() => Promise.all([original, rewritten].map((root) => rm(root, { recursive: true, force: true }))));
  const git = (root, ...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
  for (const root of [original, rewritten]) {
    git(root, "init", "--quiet");
    git(root, "config", "user.name", "Snapshot test");
    git(root, "config", "user.email", "snapshot@example.invalid");
    git(root, "config", "commit.gpgsign", "false");
    git(root, "config", "core.hooksPath", "/dev/null");
    await mkdir(join(root, "frontend/src"), { recursive: true });
    await mkdir(join(root, "website/snapshot"), { recursive: true });
    await writeFile(join(root, "frontend/src/app.ts"), "真实源码\n");
    await writeFile(join(root, "frontend/src/raw.bin"), Buffer.from([0, 128, 255, 10]));
    await symlink("app.ts", join(root, "frontend/src/current.ts"));
    git(root, "add", ".");
    git(root, "commit", "--quiet", "-m", root === original ? "Original source" : "Initialize project");
  }
  const revision = git(original, "rev-parse", "HEAD");
  const sourceState = captureSourceState(original, revision);
  await writeFile(join(rewritten, "website/snapshot/data.json"), JSON.stringify({ revision, sourceState }));
  assert.throws(() => git(rewritten, "cat-file", "-e", `${revision}^{commit}`));
  assert.equal((await checkSnapshotFreshness(rewritten)).fresh, true);
  assert.equal(blobHash(await readFile(join(rewritten, "frontend/src/raw.bin"))), sourceState.files["frontend/src/raw.bin"].split(" ")[1]);

  git(rewritten, "rm", "--cached", "frontend/src/app.ts");
  assert.deepEqual((await checkSnapshotFreshness(rewritten)).changed, ["frontend/src/app.ts"]);
  git(rewritten, "add", "frontend/src/app.ts");

  await chmod(join(rewritten, "frontend/src/app.ts"), 0o755);
  assert.deepEqual((await checkSnapshotFreshness(rewritten)).changed, ["frontend/src/app.ts"]);
  await chmod(join(rewritten, "frontend/src/app.ts"), 0o644);
  await rm(join(rewritten, "frontend/src/current.ts"));
  await symlink("raw.bin", join(rewritten, "frontend/src/current.ts"));
  assert.deepEqual((await checkSnapshotFreshness(rewritten)).changed, ["frontend/src/current.ts"]);
  await writeFile(join(rewritten, "frontend/src/raw.bin"), "changed");
  assert.deepEqual((await checkSnapshotFreshness(rewritten)).changed, ["frontend/src/current.ts", "frontend/src/raw.bin"]);
});
