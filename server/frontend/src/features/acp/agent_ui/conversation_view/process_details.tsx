import { t } from '../../../../lib/i18n';
import { useProcessSync } from '../../sync/process';
import type { Data, SessionSnapshot, TurnOverview } from '../../types';
import { messageGroups } from './entry_view_state';
import { useEntryRenderer } from './entry_renderer';
import type { OpenFile } from './content';
import { ProcessTimeline } from './process_timeline';
import { processSegments } from './conversation_turns';

export interface RemoteProcess {
  session: SessionSnapshot; turn: TurnOverview; busy: boolean;
  onAction: (action: Data) => Promise<boolean>; onOpenFile: OpenFile;
}

// Expanded segments share their turn's downloaded process and sync cursor;
// each mounts only its own terminal/editor resources.
export function ProcessDetails({ session, turn, busy, onAction, onOpenFile, segmentId }: RemoteProcess & { segmentId: string }) {
  const { data: details, error, reload } = useProcessSync(session.thread_id, turn.id, session.sync_generation, turn.revision);
  return <>
    {error && <div className="acp-content"><p className="acp-error" role="alert">{error}</p><button type="button" onClick={reload}>{t("acp.retry_loading_activity")}</button></div>}
    {!details && !error && <p className="acp-content acp-muted" role="status">{t("acp.loading_activity")}</p>}
    {details && <LoadedProcess session={details} turn={turn} busy={busy} onAction={onAction} onOpenFile={onOpenFile} segmentId={segmentId} />}
  </>;
}

function LoadedProcess({ session, turn, busy, onAction, onOpenFile, segmentId }: RemoteProcess & { segmentId: string }) {
  const permissionTools = new Set(session.permissions.map(permission => String((permission.request.toolCall as Data | undefined)?.toolCallId ?? '')));
  const finalIds = new Set(turn.conclusion.map(entry => entry.id));
  const groups = messageGroups(session.entries, turn.running, turn.waiting, permissionTools)
    .filter(group => group.kind !== 'user').flatMap(group => {
      const entries = group.entries.filter(entry => !finalIds.has(entry.id));
      return entries.length ? [{ ...group, entries }] : [];
    });
  const process = processSegments(groups, turn.id, permissionTools).find(segment => segment.id === segmentId)?.process ?? [];
  const renderGroup = useEntryRenderer({ ...session, status: turn.running ? 'working' : 'idle' }, busy, onAction, onOpenFile);
  return <ProcessTimeline process={process} running={turn.running} permissionTools={permissionTools} renderGroup={group => renderGroup(group)} />;
}
