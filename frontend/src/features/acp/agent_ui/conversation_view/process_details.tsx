import { useEffect, useRef, useState } from 'react';
import { acpApi } from '../../api';
import { applySessionUpdates } from '../../session_updates';
import { failure, type Data, type SessionSnapshot, type TurnOverview } from '../../types';
import { messageGroups } from './entry_view_state';
import { useEntryRenderer } from './entry_renderer';
import type { OpenFile } from './content';
import { ProcessTimeline } from './process_timeline';

export interface RemoteProcess {
  session: SessionSnapshot; turn: TurnOverview; busy: boolean;
  onAction: (action: Data) => Promise<boolean>; onOpenFile: OpenFile;
}

// This component exists only while expanded. It is the sole owner of the
// downloaded process, cursors, requests and mounted terminal/editor resources.
export function ProcessDetails({ session, turn, busy, onAction, onOpenFile }: RemoteProcess) {
  const [details, setDetails] = useState<SessionSnapshot>();
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  const wanted = useRef(turn.revision);
  wanted.current = turn.revision;
  const refresh = useRef<() => void>(() => undefined);
  const generation = session.sync_generation;
  const threadId = session.thread_id;
  const turnId = turn.id;
  useEffect(() => {
    const controller = new AbortController();
    let current: SessionSnapshot | undefined;
    let fetching = false;
    setDetails(undefined); setError('');
    const read = async () => {
      if (fetching || controller.signal.aborted || current && current.revision >= wanted.current) return;
      fetching = true;
      try {
        do {
          const previousRevision = current?.revision;
          const update = await acpApi.process(threadId, turnId, current, controller.signal);
          if (controller.signal.aborted) return;
          let next = applySessionUpdates(current, update);
          if (!next) next = applySessionUpdates(undefined, await acpApi.process(threadId, turnId, undefined, controller.signal));
          if (controller.signal.aborted) return;
          if (!next || next.sync_generation !== generation) throw new Error('会话已更新，请重新展开处理过程。');
          current = next;
          setDetails(next); setError('');
          if (next.revision === previousRevision) break;
        } while (current.revision < wanted.current && !controller.signal.aborted);
      } catch (reason) { if (!controller.signal.aborted) setError(failure(reason)); }
      finally { fetching = false; }
    };
    refresh.current = () => { void read(); };
    refresh.current();
    return () => { controller.abort(); refresh.current = () => undefined; current = undefined; };
  }, [threadId, turnId, generation, attempt]);
  useEffect(() => { refresh.current(); }, [turn.revision]);
  return <>
    {error && <div className="acp-content"><p className="acp-error" role="alert">{error}</p><button type="button" onClick={() => setAttempt(value => value + 1)}>重试加载处理过程</button></div>}
    {!details && !error && <p className="acp-content acp-muted" role="status">正在加载处理过程…</p>}
    {details && <LoadedProcess session={details} turn={turn} busy={busy} onAction={onAction} onOpenFile={onOpenFile} />}
  </>;
}

function LoadedProcess({ session, turn, busy, onAction, onOpenFile }: RemoteProcess) {
  const permissionTools = new Set(session.permissions.map(permission => String((permission.request.toolCall as Data | undefined)?.toolCallId ?? '')));
  const finalIds = new Set(turn.conclusion.map(entry => entry.id));
  const process = messageGroups(session.entries, turn.running, turn.waiting, permissionTools)
    .filter(group => group.kind !== 'user').flatMap(group => {
      const entries = group.entries.filter(entry => !finalIds.has(entry.id));
      return entries.length ? [{ ...group, entries }] : [];
    });
  const renderGroup = useEntryRenderer({ ...session, status: turn.running ? 'working' : 'idle' }, busy, onAction, onOpenFile);
  return <ProcessTimeline process={process} running={turn.running} permissionTools={permissionTools} renderGroup={group => renderGroup(group)} />;
}
