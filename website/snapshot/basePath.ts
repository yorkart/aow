// Snapshot-only storage adapter. Production components keep their existing API.
export const basePath = new URL(".", window.location.href).pathname.replace(
  /\/$/,
  "",
);
export function appUrl(path: string) {
  return basePath + path;
}
export function appPath(path: string) {
  return path.startsWith(basePath)
    ? path.slice(basePath.length) || "/"
    : undefined;
}
export const appBaseUrl = () => window.location.origin + basePath;
export const storageKey = (key: string) => `aow-snapshot:${basePath}:${key}`;
function memoryStorage() {
  const items = new Map<string, string>();
  return {
    getItem: (key: string) => items.get(key) ?? null,
    setItem: (key: string, value: string) => {
      items.set(key, value);
    },
    removeItem: (key: string) => {
      items.delete(key);
    },
  };
}
export const appLocalStorage = memoryStorage();
export const appSessionStorage = memoryStorage();
appLocalStorage.setItem("aow-left-width", "180");
appLocalStorage.setItem("aow-right-width", "260");
