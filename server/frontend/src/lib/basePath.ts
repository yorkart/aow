// Supplied by the web server before any module runs. Vite development/test
// pages have no metadata and keep using the root deployment.
export const basePath = typeof document === 'undefined' ? ''
  : document.querySelector<HTMLMetaElement>('meta[name="aow-base-path"]')?.content ?? '';

/** Convert a root-relative application URL into a deployment URL, exactly once. */
export function appUrl(path: string): string {
  if (!path.startsWith('/') || path.startsWith('//')) throw new Error('Expected a root-relative application URL');
  return basePath + path;
}

export function appPath(pathname: string): string | undefined {
  if (!basePath) return pathname;
  if (pathname === basePath) return '/';
  return pathname.startsWith(basePath + '/') ? pathname.slice(basePath.length) : undefined;
}

export const appBaseUrl = () => window.location.origin + basePath;

// Preserve existing root-deployment state; isolate subpath deployments on the
// same origin. This is namespacing, not a security boundary between sites.
export const storageKey = (key: string) => basePath ? `aow@${basePath}:${key}` : key;
function scopedStorage(kind: 'localStorage' | 'sessionStorage') {
  return {
    getItem: (key: string) => window[kind].getItem(storageKey(key)),
    setItem: (key: string, value: string) => window[kind].setItem(storageKey(key), value),
    removeItem: (key: string) => window[kind].removeItem(storageKey(key)),
  };
}
export const appLocalStorage = scopedStorage('localStorage');
export const appSessionStorage = scopedStorage('sessionStorage');
