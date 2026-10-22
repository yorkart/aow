import { t } from '../../../../lib/i18n';
import { useProcessSync } from '../../sync/process';
import type { Data, SessionSnapshot, TurnOverview } from '../../types';
import { messageGroups } from './entry_view_state';
import { useEntryRenderer } from './entry_renderer';
import type { OpenFile } from './content';
import { ProcessTimeline } from './process_timeline';
import { isSteeringDialogue } from './steering_messages';

export interface RemoteProcess {
  session: SessionSnapshot; turn: TurnOverview; busy: boolean;
  onAction: (action: Data) => Promise<boolean>; onOpenFile: OpenFile;
}

// This component exists only while expanded. It is the sole owner of the
// downloaded process, cursors, requests and mounted terminal/editor resources.
export function ProcessDetails({ session, turn, busy, onAction, onOpenFile }: RemoteProcess) {
  const { data: details, error, reload } = useProcessSync(session.thread_id, turn.id, session.sync_generation, turn.revision);
  return <>
    {error && <div className="acp-content"><p className="acp-error" role="alert">{error}</p><button type="button" onClick={reload}>{t("acp.retry_loading_activity")}</button></div>}
    {!details && !error && <p className="acp-content acp-muted" role="status">{t("acp.loading_activity")}</p>}
    {details && <LoadedProcess session={details} turn={turn} busy={busy} onAction={onAction} onOpenFile={onOpenFile} />}
  </>;
}

function LoadedProcess({ session, turn, busy, onAction, onOpenFile }: RemoteProcess) {
  const permissionTools = new Set(session.permissions.map(permission => String((permission.request.toolCall as Data | undefined)?.toolCallId ?? '')));
  const finalIds = new Set(turn.conclusion.map(entry => entry.id));
  const process = messageGroups(turn.messages ? session.entries.filter(entry => !isSteeringDialogue(entry)) : session.entries, turn.running, turn.waiting, permissionTools)
    .filter(group => group.kind !== 'user').flatMap(group => {
      const entries = group.entries.filter(entry => !finalIds.has(entry.id));
      return entries.length ? [{ ...group, entries }] : [];
    });
  const renderGroup = useEntryRenderer({ ...session, status: turn.running ? 'working' : 'idle' }, busy, onAction, onOpenFile);
  return <ProcessTimeline process={process} running={turn.running} permissionTools={permissionTools} renderGroup={group => renderGroup(group)} />;
}
