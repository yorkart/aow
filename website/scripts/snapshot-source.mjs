import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { lstat, readFile, readlink } from "node:fs/promises";
import { join } from "node:path";

// Keep this scope aligned with the product build and captured source files.
export const productInputs = [
  "Cargo.toml",
  "Cargo.lock",
  "crates",
  "frontend",
  "vt-worker",
  "README.md",
  "docs",
];

const git = (root, ...args) =>
  execFileSync("git", args, {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });

export function blobHash(content) {
  const bytes = Buffer.from(content);
  return createHash("sha1")
    .update(`blob ${bytes.length}\0`)
    .update(bytes)
    .digest("hex");
}

export function captureSourceState(root, revision) {
  const files = {};
  const entries = git(root, "ls-tree", "-r", "-z", revision, "--", ...productInputs)
    .split("\0").filter(Boolean);
  for (const entry of entries) {
    const separator = entry.indexOf("\t");
    const [mode, type, hash] = entry.slice(0, separator).split(" ");
    const path = entry.slice(separator + 1);
    if (type !== "blob")
      throw new Error(`Unsupported snapshot source: ${path}`);
    files[path] = `${mode} ${hash}`;
  }
  // Match the Git API when merged branches have interleaved commit dates.
  const commits = git(root, "log", "--topo-order", "-5", "--format=%H", revision)
    .trim().split("\n");
  return { version: 1, files, commits };
}

export function snapshotSourceState(snapshot) {
  const source = snapshot.sourceState;
  if (source?.version !== 1 || !source.files ||
      typeof source.files !== "object" || Array.isArray(source.files) ||
      !Object.keys(source.files).length)
    throw new Error("Snapshot source manifest is missing or invalid. Refresh the snapshot.");
  for (const [path, entry] of Object.entries(source.files)) {
    if (path.split("/").some((part) => part === ".." || part === "." || !part) ||
        !productInputs.some((input) => path === input || path.startsWith(`${input}/`)) ||
        !/^(100644|100755|120000) [a-f0-9]{40}$/.test(entry))
      throw new Error(`Invalid snapshot source entry: ${path}`);
  }
  if (!Array.isArray(source.commits) || source.commits[0] !== snapshot.revision ||
      source.commits.some((id) => !/^[a-f0-9]{40}$/.test(id)))
    throw new Error("Snapshot source history is missing or invalid.");
  return source;
}

export async function workingSourceFiles(root) {
  const paths = git(root, "ls-files", "--cached", "-z", "--", ...productInputs)
    .split("\0").filter(Boolean);
  const untracked = git(root, "ls-files", "--others", "--exclude-standard", "-z", "--", ...productInputs)
    .split("\0").filter(Boolean);
  const files = {};
  for (const path of [...new Set([...paths, ...untracked])].sort()) {
    const absolute = join(root, path);
    try {
      const info = await lstat(absolute);
      if (!info.isFile() && !info.isSymbolicLink())
        throw new Error(`Unsupported snapshot source: ${path}`);
      const mode = info.isSymbolicLink() ? "120000" :
        (info.mode & 0o111) ? "100755" : "100644";
      const content = info.isSymbolicLink() ?
        await readlink(absolute) : await readFile(absolute);
      files[path] = `${mode} ${blobHash(content)}`;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
  return { files, untracked };
}
