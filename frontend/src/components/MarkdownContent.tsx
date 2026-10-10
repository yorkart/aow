import DOMPurify from 'dompurify';
import { marked, Renderer } from 'marked';
import { useEffect, useMemo, useRef } from 'react';
import { linkImageReferences } from './markdownImageReferences';

let nextMermaidDiagramId = 0;

function isMermaidCodeBlock(code: HTMLElement) {
  return [...code.classList].some((className) => {
    const language = className.toLowerCase();
    return language === 'language-mermaid'
      || language === 'lang-mermaid'
      || language === 'language-flowchart'
      || language === 'lang-flowchart';
  });
}

export function MarkdownContent({ text, className = 'project-aow-snapshot-markdown', readOnly = false, imageReferences, onOpenLink }: { text: string; className?: string; readOnly?: boolean; imageReferences?: ReadonlyMap<string, string>; onOpenLink?: (href: string) => boolean }) {
  const markdownRef = useRef<HTMLDivElement>(null);
  const handlesLinks = !!onOpenLink;
  const html = useMemo(() => {
    // Preserve file destinations as inert data only when a host handles clicks.
    // DOMPurify still applies its normal URL policy to every actual href.
    const renderer = new Renderer();
    if (handlesLinks) renderer.link = function (token) {
      if (!/^file:\/\//i.test(token.href)) return Renderer.prototype.link.call(this, token);
      return `<a href="#" data-local-file="${encodeURIComponent(token.href)}">${this.parser.parseInline(token.tokens)}</a>`;
    };
    const source = marked.parse(text, { renderer }) as string;
    if (!readOnly) return DOMPurify.sanitize(source);
    const content = document.createElement('div');
    content.append(DOMPurify.sanitize(source, {
      RETURN_DOM_FRAGMENT: true,
      FORBID_TAGS: ['form', 'button', 'textarea', 'select', 'option'],
      FORBID_ATTR: ['contenteditable', 'autofocus'],
    }));
    if (handlesLinks) content.querySelectorAll('a[href]').forEach(link => {
      if (/^https?:\/\//i.test(link.getAttribute('href') ?? '')) {
        link.setAttribute('target', '_blank'); link.setAttribute('rel', 'noreferrer');
      }
    });
    content.querySelectorAll('input').forEach((input) => {
      if (input.type === 'checkbox') input.disabled = true;
      else input.remove();
    });
    if (imageReferences?.size) linkImageReferences(content, imageReferences);
    return content.innerHTML;
  }, [readOnly, text, imageReferences, handlesLinks]);

  useEffect(() => {
    const markdown = markdownRef.current;
    if (!markdown) return;
    // This effect owns the transformed DOM. React must not restore the source
    // HTML on snapshot refreshes when the Markdown itself has not changed.
    markdown.innerHTML = html;
    const codeBlocks = [...markdown.querySelectorAll<HTMLElement>('pre > code')].filter(isMermaidCodeBlock);
    if (!codeBlocks.length) return;

    let cancelled = false;
    const diagramPrefix = `agent-session-flowchart-${nextMermaidDiagramId += 1}`;
    void import('mermaid').then(async ({ default: mermaid }) => {
      // SVG text labels survive the SVG-only sanitization below.
      mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: 'dark', htmlLabels: false });
      await Promise.all(codeBlocks.map(async (code, index) => {
        try {
          const { svg, bindFunctions } = await mermaid.render(`${diagramPrefix}-${index}`, code.textContent ?? '');
          if (cancelled || !code.isConnected) return;
          const diagram = document.createElement('div');
          diagram.className = 'project-aow-snapshot-flowchart';
          diagram.innerHTML = DOMPurify.sanitize(svg, {
            USE_PROFILES: { svg: true, svgFilters: true },
          });
          bindFunctions?.(diagram);
          code.parentElement?.replaceWith(diagram);
        } catch {
          if (cancelled || !code.isConnected) return;
          const error = document.createElement('p');
          error.className = 'project-aow-snapshot-flowchart-error';
          error.textContent = 'Flowchart 渲染失败，已保留原始代码。';
          code.parentElement?.after(error);
        }
      }));
    }).catch(() => {
      if (cancelled || !markdown.isConnected) return;
      const error = document.createElement('p');
      error.className = 'project-aow-snapshot-flowchart-error';
      error.textContent = 'Flowchart 渲染组件加载失败，已保留原始代码。';
      markdown.append(error);
    });

    return () => { cancelled = true; };
  }, [html]);

  return <div ref={markdownRef} className={className} onClick={event => {
    if (!onOpenLink || !(event.target instanceof Element)) return;
    const link = event.target.closest('a');
    if (!link || !event.currentTarget.contains(link)) return;
    let href = link.getAttribute('href');
    try { if (link.dataset.localFile) href = decodeURIComponent(link.dataset.localFile); } catch { return; }
    if (href && onOpenLink(href)) event.preventDefault();
  }} />;
}
