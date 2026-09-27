import assert from "node:assert/strict";
import { test } from "node:test";
import { shouldCaptureCommitDiff } from "../scripts/capture-policy.mjs";

test("repeated releases do not record previous snapshot payloads while normal product changes remain visible", () => {
  const files = [
    { path: "website/snapshot/data.json" },
    {
      path: "website/snapshot/old-data.json",
      original_path: "website/snapshot/data.json",
    },
    { path: "website/_snapshot/data.json" },
    { path: "website/_site/snapshot/data.json" },
    { path: "website/scripts/capture-snapshot.mjs" },
    { path: "frontend/src/snapshot/data.json" },
    { path: "frontend/src/aow/ProjectAow.tsx" },
  ];
  const original = structuredClone(files);
  assert.deepEqual(
    files.filter(shouldCaptureCommitDiff).map((file) => file.path),
    [
      "website/scripts/capture-snapshot.mjs",
      "frontend/src/snapshot/data.json",
      "frontend/src/aow/ProjectAow.tsx",
    ],
  );
  assert.deepEqual(
    files,
    original,
    "the actual commit file list must be preserved",
  );
});
