import { useMemo } from 'react';
import DOMPurify from 'dompurify';
import { marked } from 'marked';
import { parseDocument } from 'yaml';
import { dataUrl, relativePath } from './files';
import type { SkillEntry, SkillFile } from './types';

export function MarkdownPreview({ file, files, onOpen }: { file: SkillFile; files: SkillEntry[]; onOpen: (path: string) => void }) {
  const preview = useMemo(() => {
    let body = file.content;
    let metadata: { name: string; description: string } | undefined;
    let error = '';
    if (file.path === 'SKILL.md') {
      const match = /^(?:\uFEFF)?---\r?\n([\s\S]*?)\r?\n---(?:\r?\n|$)/.exec(body);
      if (match) {
        body = body.slice(match[0].length);
        try {
          const document = parseDocument(match[1], { uniqueKeys: true });
          if (document.errors.length) throw new Error();
          const value: unknown = document.toJS({ maxAliasCount: 0 });
          if (!value || typeof value !== 'object' || !('name' in value) || !('description' in value) || typeof value.name !== 'string' || typeof value.description !== 'string') throw new Error();
          metadata = { name: value.name, description: value.description };
        } catch { error = 'SKILL.md 元数据格式无效，保存前请检查 name、description 和 YAML 格式。'; }
      } else error = 'SKILL.md 缺少 YAML frontmatter，保存前请补充 name 和 description。';
    }
    const sanitized = DOMPurify.sanitize(marked.parse(body, { async: false }), { FORBID_TAGS: ['style'], FORBID_ATTR: ['style'] });
    const document = new DOMParser().parseFromString(sanitized, 'text/html');
    for (const image of document.querySelectorAll('img')) {
      const path = relativePath(file.path, image.getAttribute('src') ?? '');
      const entry = files.find(item => item.path === path);
      if (entry?.kind === 'file') image.setAttribute('src', dataUrl(entry));
    }
    for (const anchor of document.querySelectorAll('a')) {
      if (/^https?:/i.test(anchor.getAttribute('href') ?? '')) { anchor.target = '_blank'; anchor.rel = 'noopener noreferrer'; }
    }
    return { html: document.body.innerHTML, metadata, error };
  }, [file, files]);
  return <article className="skills-markdown" aria-label="Markdown 预览" onClick={event => {
    const anchor = event.target instanceof Element ? event.target.closest('a') : null;
    if (!anchor) return;
    const href = anchor.getAttribute('href') ?? '';
    const path = relativePath(file.path, href);
    if (path !== undefined) { event.preventDefault(); onOpen(path); }
    else if (!/^https?:|^mailto:|^#/i.test(href)) event.preventDefault();
  }}>{preview.error && <p className="skills-error" role="alert">{preview.error}</p>}{preview.metadata && <header className="skills-entry-metadata"><h1>{preview.metadata.name}</h1><p>{preview.metadata.description}</p></header>}<div dangerouslySetInnerHTML={{ __html: preview.html }} /></article>;
}
