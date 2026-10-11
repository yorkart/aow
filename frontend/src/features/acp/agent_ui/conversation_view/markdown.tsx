// MarkdownFont::Agent and the default code-block controls, using the host's sanitizer.
import { memo, useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { WrapText } from 'lucide-react';
import { MarkdownContent } from '../../../../components/MarkdownContent';
import { CopyButton } from '../ui/copy_button';
import { localFileLink } from './file_links';
import type { OpenFile } from './content';

function CodeActions({ container, source }: { container: HTMLElement; source: string }) {
  const [wrap, setWrap] = useState(false);
  useEffect(() => { container.classList.toggle('acp-code-wrap', wrap); }, [container, wrap]);
  return createPortal(<span className="acp-code-actions"><CopyButton text={source} label="复制代码" /><button type="button" className="acp-quiet-button" aria-label="代码自动换行" title="代码自动换行" aria-pressed={wrap} onClick={() => setWrap(value => !value)}><WrapText size={13} /></button></span>, container);
}

export const AgentMarkdown = memo(function AgentMarkdown({ text, onOpenFile, controls = true, message = false }: { text: string; onOpenFile: OpenFile; controls?: boolean; message?: boolean }) {
  const root = useRef<HTMLDivElement>(null);
  const nextId = useRef(0);
  const [blocks, setBlocks] = useState<{ id: number; pre: HTMLElement; source: string }[]>([]);
  useEffect(() => {
    const element = root.current;
    if (!element || !controls) return;
    const refresh = () => {
      const elements = [...element.querySelectorAll<HTMLElement>('pre')].filter(pre => pre.querySelector(':scope > code'));
      setBlocks(previous => {
        const next = elements.map((pre, index) => {
          const old = previous.find(block => block.pre === pre) ?? previous[index];
          const source = pre.querySelector(':scope > code')?.textContent ?? '';
          return old?.pre === pre && old.source === source ? old : { id: old?.id ?? ++nextId.current, pre, source };
        });
        return next.length === previous.length && next.every((block, index) => block === previous[index]) ? previous : next;
      });
    };
    const observer = new MutationObserver(refresh);
    observer.observe(element, { childList: true, subtree: true, characterData: true });
    refresh();
    return () => observer.disconnect();
  }, [controls]);
  return <div className="acp-markdown" ref={root}>
    <MarkdownContent text={text} className={message ? 'project-aow-snapshot-markdown conversation-prose' : undefined} readOnly breaks onOpenLink={href => { const file = localFileLink(href); if (!file) return false; onOpenFile(file.path, file.line); return true; }} />
    {controls && blocks.map(block => <CodeActions key={block.id} container={block.pre} source={block.source} />)}
  </div>;
});
