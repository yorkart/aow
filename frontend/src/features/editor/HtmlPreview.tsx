import { useEffect, useState } from 'react';
import { filesApi } from '../files/api';
import { t } from '../../lib/i18n';

async function dataUrl(blob: Blob) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

async function cssUrls(css: string, resolve: (value: string) => Promise<string>) {
  const pattern = /url\(\s*(?:"([^"\\]*(?:\\.[^"\\]*)*)"|'([^'\\]*(?:\\.[^'\\]*)*)'|([^\s)'"\\]+))\s*\)/gi;
  const replacements = await Promise.all([...css.matchAll(pattern)].map(async match => {
    const value = match[1] ?? match[2] ?? match[3];
    if (value.startsWith('#') || value.includes('\\')) return match[0];
    return `url(${JSON.stringify(await resolve(value))})`;
  }));
  let index = 0;
  return css.replace(pattern, () => replacements[index++]);
}

async function previewDocument(content: string, path: string, signal: AbortSignal) {
  const document = new DOMParser().parseFromString(content, 'text/html');
  const policy = document.createElement('meta');
  policy.httpEquiv = 'Content-Security-Policy';
  // Inline scripts can drive a standalone page, but must not call authenticated
  // application APIs. The iframe also has an opaque origin and no form access.
  policy.content = `default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline' data: http: https:; img-src data: http: https:; font-src data: http: https:; media-src data: http: https:; connect-src 'none'; form-action 'none'; base-uri ${window.location.origin}`;
  // Resolve relative styles and media against the file rather than the tab URL.
  for (const base of document.querySelectorAll('base')) base.remove();
  const base = document.createElement('base');
  base.href = new URL(filesApi.rawUrl(path), window.location.origin).href;
  document.head.prepend(policy, base);

  const rawRoot = new URL(filesApi.rawUrl('/'), window.location.origin);
  const resources = new Map<string, Promise<string>>();
  const resourceUrl = async (value: string, from: string, ancestors: string[] = []): Promise<string> => {
    const url = new URL(value, from);
    if (url.origin !== rawRoot.origin || !url.pathname.startsWith(rawRoot.pathname)) return url.href;
    const key = url.origin + url.pathname;
    if (ancestors.includes(key) || ancestors.length >= 8) return 'data:,';
    const cacheKey = JSON.stringify([...ancestors, key]);
    let resource = resources.get(cacheKey);
    if (!resource) {
      resource = (async () => {
        const path = decodeURIComponent(url.pathname.slice(rawRoot.pathname.length - 1));
        let blob = await filesApi.readBlob(path, signal);
        if (blob.type.startsWith('text/css')) {
          const css = await cssUrls(await blob.text(), value => resourceUrl(value, url.href, [...ancestors, key]));
          blob = new Blob([css], { type: 'text/css' });
        }
        signal.throwIfAborted();
        return dataUrl(blob);
      })();
      resources.set(cacheKey, resource);
    }
    return await resource + url.hash;
  };
  // Opaque sandbox origins cannot send the app's SameSite=Strict session cookie.
  // Read local assets in the parent and give the preview only embedded data URLs.
  await Promise.all([...document.querySelectorAll('img[src], audio[src], video[src], source[src], video[poster], link[rel="stylesheet"][href]')].map(async element => {
    const attributes = element.tagName === 'LINK' ? ['href'] : ['src', 'poster'];
    await Promise.all(attributes.map(async attribute => {
      const value = element.getAttribute(attribute);
      if (!value) return;
      try { element.setAttribute(attribute, await resourceUrl(value, base.href)); }
      catch { /* Keep unavailable assets as broken resources without hiding the page. */ }
    }));
  }));
  await Promise.all([...document.querySelectorAll('style, [style]')].map(async element => {
    const attribute = element.tagName !== 'STYLE';
    const css = attribute ? element.getAttribute('style')! : element.textContent ?? '';
    try {
      const resolved = await cssUrls(css, value => resourceUrl(value, base.href));
      if (attribute) element.setAttribute('style', resolved);
      else element.textContent = resolved;
    } catch { /* Unavailable CSS assets must not hide the rest of the document. */ }
  }));
  return `<!doctype html>\n${document.documentElement.outerHTML}`;
}

export function HtmlPreview({ content, path, name }: { content: string; path: string; name: string }) {
  const [preview, setPreview] = useState<{ content: string; path: string; source: string }>();
  useEffect(() => {
    const controller = new AbortController();
    void previewDocument(content, path, controller.signal).then(source => {
      if (!controller.signal.aborted) setPreview({ content, path, source });
    });
    return () => {
      controller.abort();
    };
  }, [content, path]);
  const source = preview?.content === content && preview.path === path ? preview.source : '';
  if (!source) return <div className="editor-loading"><span className="loading-spinner" />{t("automations.loading")}</div>;
  return <iframe className="html-preview" title={name} sandbox="allow-scripts" referrerPolicy="no-referrer" srcDoc={source} />;
}
