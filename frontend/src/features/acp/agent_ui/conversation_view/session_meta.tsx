import { useEffect, useState } from 'react';
import type { Data } from '../../types';
import { Notice } from './activity';

const duration = 10_000;
type Hint = { token: string; expiresAt: number };
// Expiry belongs to the session/key, so replacing the composer cannot revive old hints.
// Keep a bounded history of recently viewed sessions across portal and tab mounts.
const sessions = new Map<string, Map<string, Hint>>();
function currentHint(sessionId: string, name: string, token: string) {
  let hints = sessions.get(sessionId);
  if (!hints) {
    hints = new Map();
    sessions.set(sessionId, hints);
    if (sessions.size > 256) sessions.delete(sessions.keys().next().value!);
  }
  let hint = hints.get(name);
  if (!hint || hint.token !== token) {
    hint = { token, expiresAt: Date.now() + duration };
    hints.set(name, hint);
  }
  return hint;
}

export function SessionMeta({ sessionId, metadata, revisions }: { sessionId: string; metadata?: Data; revisions?: Record<string, number> }) {
  if (!metadata) return null;
  return <section className="acp-session-meta" aria-label="会话元数据">
    {Object.entries(metadata).map(([name, value]) => <MetadataHint key={name} sessionId={sessionId} name={name} value={value} revision={revisions?.[name] ?? 0} />)}
  </section>;
}

function MetadataHint({ sessionId, name, value, revision }: { sessionId: string; name: string; value: unknown; revision: number }) {
  const serialized = JSON.stringify(value);
  const hint = currentHint(sessionId, name, `${revision}:${serialized}`);
  const [, refresh] = useState(0);
  const [hovered, setHovered] = useState(false);
  const [focused, setFocused] = useState(false);
  const paused = hovered || focused;
  const expiresAt = hint.expiresAt;
  const visible = paused || expiresAt > Date.now();
  useEffect(() => {
    if (!visible || paused) return;
    const timer = setTimeout(() => refresh(count => count + 1), Math.max(0, expiresAt - Date.now()));
    return () => clearTimeout(timer);
  }, [expiresAt, paused, visible]);
  if (!visible) return null;
  return <div tabIndex={0}
    onMouseEnter={() => setHovered(true)} onMouseLeave={() => { hint.expiresAt = Date.now() + duration; setHovered(false); }}
    onFocus={() => setFocused(true)} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) { hint.expiresAt = Date.now() + duration; setFocused(false); } }}>
    <Notice notice={{ id: name, kind: 'notice', content: { title: `${name} meta:`, severity: 'info' } }}>
      <span>{`${name} meta:`}</span> <code>{serialized}</code>
    </Notice>
  </div>;
}
