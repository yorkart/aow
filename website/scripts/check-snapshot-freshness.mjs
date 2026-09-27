import { readFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { snapshotSourceState, workingSourceFiles } from "./snapshot-source.mjs";

const defaultRoot = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

export async function checkSnapshotFreshness(root = defaultRoot) {
  const snapshot = JSON.parse(
    await readFile(join(root, "website/snapshot/data.json"), "utf8"),
  );
  if (!/^[a-f0-9]{40}$/.test(snapshot.revision ?? ""))
    throw new Error("Snapshot is missing a valid source revision");
  // Content fingerprints survive squash/rebase and shallow checkouts.
  const expected = snapshotSourceState(snapshot).files;
  const { files: current, untracked } = await workingSourceFiles(root);
  const changed = [...new Set([...Object.keys(expected), ...Object.keys(current)])]
    .filter((path) => expected[path] !== current[path] || untracked.includes(path))
    .sort();
  return { revision: snapshot.revision, fresh: changed.length === 0, changed };
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  try {
    const result = await checkSnapshotFreshness();
    if (result.fresh) {
      console.log(
        `Snapshot ${result.revision.slice(0, 7)} matches the current product sources.`,
      );
    } else {
      console.error(
        `Portal snapshot is stale (${result.revision.slice(0, 7)}). Product sources changed:\n${result.changed.map((path) => `  ${path}`).join("\n")}\nRun $aow-portal-snapshot with the intended product source to refresh the portal snapshot.`,
      );
      process.exitCode = 1;
    }
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
