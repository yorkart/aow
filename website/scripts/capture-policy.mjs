// Snapshot payloads contain previous Git diffs. Recording their diffs again
// would recursively embed old captures and grow the portal every release.
export function shouldCaptureCommitDiff(file) {
  const generated = (path) =>
    path === "website/snapshot/data.json" ||
    path?.startsWith("website/_snapshot/") ||
    path?.startsWith("website/_site/");
  return !generated(file.path) && !generated(file.original_path);
}
