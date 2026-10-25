import { t } from '../../../../lib/i18n';
import { Fragment, useMemo, type ComponentProps, type ReactNode } from 'react';
import { CircleAlert } from 'lucide-react';
import { ConversationProcess, useProcessExpansion } from '../../../../components/conversation/ConversationProcess';
import { overviewPresentation, type PresentedSegment, type turnPresentation } from './conversation_turns';
import type { MessageGroup } from './entry_view_state';
import { ReplyActions } from './reply_actions';
import { ProcessDetails, type RemoteProcess } from './process_details';
import { ProcessTimeline } from './process_timeline';

export function OverviewResponse(props: Omit<ComponentProps<typeof TurnResponse>, 'presentation'> & { remote: RemoteProcess }) {
  const presentation = useMemo(() => overviewPresentation(props.remote.turn), [props.remote.turn]);
  return <TurnResponse {...props} presentation={presentation} />;
}

export function TurnResponse({ presentation, permissionTools, renderGroup, generating, onJump, onScrollToTop, remote }: {
  presentation: ReturnType<typeof turnPresentation>; permissionTools: Set<string>; renderGroup: (group: MessageGroup) => ReactNode;
  remote?: RemoteProcess; generating?: ReactNode; onJump?: () => void; onScrollToTop?: () => void;
}) {
  const { conclusion, segments, running, status, reply, error } = presentation;
  const pending = status === 'failed' ? t("acp.this_turn_failed_without_a_final_response")
    : status === 'interrupted' ? t("acp.this_turn_was_interrupted_without_a_final_response")
      : t("acp.no_recognizable_final_response_was_recorded_for_this_turn");
  return <div className="acp-turn-response">
    {segments.map(segment => <Fragment key={segment.id}>
      {segment.steering && renderGroup(segment.steering)}
      {!!(segment.tool_count + segment.progress_count) && <TurnProcess segment={segment} running={running} permissionTools={permissionTools} renderGroup={renderGroup} remote={remote} />}
    </Fragment>)}
    {!!conclusion.length && <div className="project-aow-snapshot-conclusion">{conclusion.map(renderGroup)}</div>}
    {generating}
    {!running && !conclusion.length && <p className={`acp-turn-pending ${status ?? 'unknown'}`} role="status"><CircleAlert size={14} />{pending}{error && <span>{error}</span>}</p>}
    {!running && onJump && <div className="acp-content"><ReplyActions content={reply} onJump={onJump} onScrollToTop={onScrollToTop} /></div>}
  </div>;
}

function TurnProcess({ segment, running, permissionTools, renderGroup, remote }: {
  segment: PresentedSegment; running: boolean; permissionTools: Set<string>; renderGroup: (group: MessageGroup) => ReactNode; remote?: RemoteProcess;
}) {
  const required = segment.permission_tool_ids.length > 0;
  const { expanded, toggle } = useProcessExpansion(running, required);
  const { tool_count: tools, progress_count: progress } = segment;
  return <ConversationProcess expanded={expanded} onToggle={toggle} required={required}
    summary={[tools ? t("acp.tool_calls", { count: tools, value0: tools }) : '', progress ? t("acp.progress_updates", { value0: progress }) : ''].filter(Boolean).join(' · ')}>
    {expanded && (remote ? <ProcessDetails key={`${remote.session.sync_generation}:${remote.turn.id}`} {...remote} segmentId={segment.id} />
      : <ProcessTimeline process={segment.process} running={running} permissionTools={permissionTools} renderGroup={renderGroup} />)}
  </ConversationProcess>;
}
