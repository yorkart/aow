import { build } from 'esbuild';
import { fileURLToPath } from 'node:url';

// Bundle real module dependencies rather than transpiling into a data URL with
// unresolved relative imports. Existing unit fixtures explicitly exercise Chinese.
export async function loadModule(url, language = 'zh-CN', { blockedStorage = false } = {}) {
  const storage = Object.getOwnPropertyDescriptor(globalThis, 'localStorage');
  Object.defineProperty(globalThis, 'localStorage', { configurable: true, value: {
    getItem: () => { if (blockedStorage) throw new Error('storage blocked'); return language; },
    setItem: () => { if (blockedStorage) throw new Error('storage blocked'); },
  } });
  try {
    const result = await build({ entryPoints: [fileURLToPath(url)], bundle: true, write: false,
      platform: 'node', format: 'esm', target: 'es2022', logLevel: 'silent' });
    return await import(`data:text/javascript;base64,${Buffer.from(result.outputFiles[0].text).toString('base64')}#${encodeURIComponent(language)}-${blockedStorage}`);
  } finally {
    if (storage) Object.defineProperty(globalThis, 'localStorage', storage);
    else delete globalThis.localStorage;
  }
}
