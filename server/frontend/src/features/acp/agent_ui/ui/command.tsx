import { t } from '../../../../lib/i18n';
import { useMemo, type ReactNode } from 'react';
import { refractor } from 'refractor/core';
import bash from 'refractor/bash';
import { CopyButton } from './copy_button';

refractor.register(bash);

type Tokens = ReturnType<typeof refractor.highlight>['children'];

function renderTokens(nodes: Tokens): ReactNode {
  return nodes.map((node, index) => {
    if (node.type === 'text') return node.value;
    if (node.type !== 'element') return null;
    // Only React text and spans: shell strings must never become HTML or links.
    const classes = node.properties.className;
    return <span key={index} className={Array.isArray(classes) ? classes.join(' ') : undefined}>{renderTokens(node.children)}</span>;
  });
}

export function Command({ title }: { title: string }) {
  const highlighted = useMemo(() => {
    // Keep unusually large adapter payloads readable without tokenizing them.
    if (title.length > 32_768) return title;
    try { return renderTokens(refractor.highlight(title, 'bash').children); }
    catch { return title; }
  }, [title]);
  return <div className="acp-command"><pre className="acp-command-code"><code>{highlighted}</code></pre><CopyButton text={title} label={t("acp.copy_command")} /></div>;
}
