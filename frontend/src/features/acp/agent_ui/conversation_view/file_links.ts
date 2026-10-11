// ACP paths are resolved against the agent's cwd, not the browser's URL.
export function localFileLink(href: string): { path: string; line?: number } | undefined {
  if (!href || href.startsWith('#') || href.startsWith('//')) return;
  try {
    let path = href;
    if (/^file:\/\//i.test(href)) {
      const url = new URL(href);
      if (url.hostname && url.hostname !== 'localhost') return;
      path = `${url.pathname}${url.hash}`;
    }
    const line = /(?::(\d+)(?::\d+)?|#L?(\d+)(?:C\d+)?(?:-L?\d+)?)$/.exec(path);
    path = decodeURIComponent(line ? path.slice(0, line.index) : path.split('#')[0]);
    if (!path || [...path].some(char => char.charCodeAt(0) < 32) || /^[a-z][a-z\d+.-]*:/i.test(path)) return;
    const number = line ? Number(line[1] || line[2]) : undefined;
    return { path, line: number && Number.isSafeInteger(number) ? number : undefined };
  } catch { return; }
}

export function absoluteFilePath(path: string, cwd: string): string {
  const parts: string[] = [];
  for (const part of (path.startsWith('/') ? path : `${cwd}/${path}`).split('/')) {
    if (part === '..') parts.pop();
    else if (part && part !== '.') parts.push(part);
  }
  return `/${parts.join('/')}`;
}
