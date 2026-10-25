import { t } from '../lib/i18n';
import DOMPurify from 'dompurify';
import { marked, Renderer } from 'marked';
import { useEffect, useMemo, useRef } from 'react';
import { linkImageReferences } from './markdownImageReferences';
import { useTheme } from '../lib/theme/useTheme';

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

export function MarkdownContent({ text, className = 'project-aow-snapshot-markdown', readOnly = false, breaks = false, imageReferences, onOpenLink, resolveLink }: { text: string; className?: string; readOnly?: boolean; breaks?: boolean; imageReferences?: ReadonlyMap<string, string>; onOpenLink?: (href: string) => boolean; resolveLink?: (href: string) => string }) {
  const theme = useTheme();
  const markdownRef = useRef<HTMLDivElement>(null);
  const handlesLinks = !!onOpenLink;
  const html = useMemo(() => {
    // Route file destinations before sanitization, or preserve them as inert
    // data for a host click handler. DOMPurify still checks every actual href.
    const renderer = new Renderer();
    if (handlesLinks || resolveLink) renderer.link = function (token) {
      const resolved = resolveLink?.(token.href);
      if (resolved && resolved !== token.href) return Renderer.prototype.link.call(this, { ...token, href: resolved });
      if (!handlesLinks || !/^file:\/\//i.test(token.href)) return Renderer.prototype.link.call(this, token);
      return `<a href="#" data-local-file="${encodeURIComponent(token.href)}">${this.parser.parseInline(token.tokens)}</a>`;
    };
    const source = marked.parse(text, { renderer, breaks }) as string;
    if (!readOnly && !resolveLink) return DOMPurify.sanitize(source);
    const content = document.createElement('div');
    content.append(DOMPurify.sanitize(source, {
      RETURN_DOM_FRAGMENT: true,
      FORBID_TAGS: readOnly ? ['form', 'button', 'textarea', 'select', 'option'] : [],
      FORBID_ATTR: readOnly ? ['contenteditable', 'autofocus'] : [],
    }));
    // Transform only sanitized destinations, including links authored as HTML.
    if (resolveLink) content.querySelectorAll('a[href]').forEach(link => {
      link.setAttribute('href', resolveLink(link.getAttribute('href')!));
    });
    if (handlesLinks || resolveLink) content.querySelectorAll('a[href]').forEach(link => {
      if (/^https?:\/\//i.test(link.getAttribute('href')?.trim() ?? '')) {
        link.setAttribute('target', '_blank'); link.setAttribute('rel', 'noreferrer');
      }
    });
    if (readOnly) content.querySelectorAll('input').forEach((input) => {
      if (input.type === 'checkbox') input.disabled = true;
      else input.remove();
    });
    if (imageReferences?.size) linkImageReferences(content, imageReferences);
    return content.innerHTML;
  }, [readOnly, text, breaks, imageReferences, handlesLinks, resolveLink]);

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
      mermaid.initialize({
        startOnLoad: false, securityLevel: 'strict', theme: 'base', htmlLabels: false,
        themeVariables: {
          darkMode: theme.appearance === 'dark', background: theme.colors.background,
          primaryColor: theme.colors.panel, primaryTextColor: theme.colors.text,
          primaryBorderColor: theme.colors.controlBorder, lineColor: theme.colors.secondary,
          secondaryColor: theme.colors.selection, tertiaryColor: theme.colors.elevated,
          edgeLabelBackground: theme.colors.panel,
        },
      });
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
          error.textContent = t("components.flowchart_rendering_failed_original_code_retained");
          code.parentElement?.after(error);
        }
      }));
    }).catch(() => {
      if (cancelled || !markdown.isConnected) return;
      const error = document.createElement('p');
      error.className = 'project-aow-snapshot-flowchart-error';
      error.textContent = t("components.unable_to_load_the_flowchart_renderer_original_code_retained");
      markdown.append(error);
    });

    return () => { cancelled = true; };
  }, [html, theme]);

  return <div ref={markdownRef} className={className} onClick={event => {
    if (event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    if (!onOpenLink || !(event.target instanceof Element)) return;
    const link = event.target.closest('a');
    if (!link || !event.currentTarget.contains(link)) return;
    let href = link.getAttribute('href');
    try { if (link.dataset.localFile) href = decodeURIComponent(link.dataset.localFile); } catch { return; }
    if (href && onOpenLink(href)) event.preventDefault();
  }} />;
}
