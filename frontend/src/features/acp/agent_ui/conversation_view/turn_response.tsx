import { useMemo, type ComponentProps, type ReactNode } from 'react';
import { CircleAlert } from 'lucide-react';
import { ConversationProcess, useProcessExpansion } from '../../../../components/conversation/ConversationProcess';
import { overviewPresentation, type turnPresentation } from './conversation_turns';
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
  const { process, conclusion, messages, running, waiting, status, reply, error } = presentation;
  const { expanded, toggle } = useProcessExpansion(running, waiting);
  const toolCount = remote?.turn.tool_count ?? process.filter(group => group.kind === 'tool').length;
  const progressCount = remote?.turn.progress_count ?? process.filter(group => group.kind !== 'tool').length;
  const pending = status === 'failed' ? '本轮执行失败，未生成最终结论。'
    : status === 'interrupted' ? '本轮已中断，未生成最终结论。'
      : '本轮未记录可识别的最终结论。';
  return <div className="acp-turn-response">
    {!!(toolCount + progressCount) && <ConversationProcess expanded={expanded} onToggle={toggle} required={waiting}
      summary={[toolCount ? `${toolCount} 次工具调用` : '', progressCount ? `${progressCount} 条进度` : ''].filter(Boolean).join(' · ')}>
      {expanded && (remote ? <ProcessDetails key={`${remote.session.sync_generation}:${remote.turn.id}`} {...remote} />
        : <ProcessTimeline process={process} running={running} permissionTools={permissionTools} renderGroup={renderGroup} />)}
    </ConversationProcess>}
    {messages ? <div className="acp-turn-dialogue">{messages.map(renderGroup)}</div>
      : !!conclusion.length && <div className="project-aow-snapshot-conclusion">{conclusion.map(renderGroup)}</div>}
    {generating}
    {!running && !conclusion.length && <p className={`acp-turn-pending ${status ?? 'unknown'}`} role="status"><CircleAlert size={14} />{pending}{error && <span>{error}</span>}</p>}
    {!running && onJump && <div className="acp-content"><ReplyActions content={reply} onJump={onJump} onScrollToTop={onScrollToTop} /></div>}
  </div>;
}
