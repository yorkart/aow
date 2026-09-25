import { createRequire } from "node:module";
import { cp, mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const require = createRequire(join(root, "frontend/package.json"));
export async function buildSnapshot(outDir) {
  const { build } = await import(require.resolve("vite"));
  const { default: react } = await import(
    require.resolve("@vitejs/plugin-react")
  );
  await build({
    configFile: false,
    root: join(root, "website/snapshot"),
    base: "./",
    publicDir: false,
    plugins: [
      react(),
      {
        name: "snapshot-storage",
        enforce: "pre",
        resolveId(source, importer) {
          if (importer && source.endsWith("lib/basePath"))
            return join(root, "website/snapshot/basePath.ts");
        },
      },
    ],
    resolve: {
      alias: [
        { find: /^react$/, replacement: require.resolve("react") },
        { find: /^react-dom$/, replacement: require.resolve("react-dom") },
        {
          find: /^react\/(.*)$/,
          replacement: join(root, "frontend/node_modules/react/$1"),
        },
        {
          find: /^react-dom\/(.*)$/,
          replacement: join(root, "frontend/node_modules/react-dom/$1"),
        },
      ],
    },
    build: {
      outDir,
      emptyOutDir: true,
      target: "es2022",
      sourcemap: false,
      chunkSizeWarningLimit: 1800,
    },
    logLevel: "warn",
  });
  await cp(join(root, "website/snapshot/data.json"), join(outDir, "data.json"));
  await cp(
    join(root, "frontend/public/workspace-icon.svg"),
    join(outDir, "workspace-icon.svg"),
  );
  // publicDir is disabled for snapshots; preserve the bundled assets' notices.
  await cp(
    join(root, "frontend/public/third-party"),
    join(outDir, "third-party"),
    { recursive: true },
  );
  // The product's download links are normal browser requests, not fetch calls.
  // Publish the recorded file bytes at those same URLs as static files.
  const snapshot = JSON.parse(
    await readFile(join(root, "website/snapshot/data.json"), "utf8"),
  );
  for (const [key, value] of Object.entries(snapshot.responses)) {
    if (!key.startsWith("/api/fs/text/workspace/aow/")) continue;
    const path = key.replace("/api/fs/text/", "api/fs/raw/");
    const target = resolve(outDir, path);
    if (!target.startsWith(resolve(outDir) + "/"))
      throw new Error("Invalid captured file path");
    await mkdir(dirname(target), { recursive: true });
    await writeFile(target, value.content);
  }
}
if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
) {
  await buildSnapshot(join(root, "website/_snapshot"));
  console.log("Built snapshot from the existing AoW frontend.");
}
