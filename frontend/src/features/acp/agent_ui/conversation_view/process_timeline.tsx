import { useId, type ReactNode } from 'react';
import { Brain, ChevronRight, MessageSquare, Wrench } from 'lucide-react';
import { useProcessExpansion } from '../../../../components/conversation/ConversationProcess';
import { groupProcess } from './conversation_turns';
import type { MessageGroup } from './entry_view_state';
import { SpinnerLabel } from '../ui/spinner_label';

function ToolGroup({ groups, running, permissionTools, renderGroup }: {
  groups: MessageGroup[]; running: boolean; permissionTools: Set<string>; renderGroup: (group: MessageGroup) => ReactNode;
}) {
  const required = groups.some(group => permissionTools.has(group.id));
  const { expanded, toggle } = useProcessExpansion(running, required);
  const contentId = useId();
  const pending = running && groups.some(group => ['pending', 'in_progress'].includes(String(group.entries[0].content.status)));
  const failed = groups.some(group => ['failed', 'rejected', 'cancelled', 'canceled'].includes(String(group.entries[0].content.status)));
  return <div className="session-tool-group acp-process-tool-group">
    <button type="button" className="session-tool-group-toggle" aria-expanded={expanded} aria-controls={contentId}
      disabled={required} aria-label={`${expanded ? '收起' : '展开'}工具执行：${groups.length} 次调用`} onClick={toggle}>
      <span className="session-tool-group-summary">工具执行</span><span className="session-tool-group-count">{groups.length} 次调用</span>
      <span className="acp-process-status">{required ? '等待授权' : pending ? <><SpinnerLabel />执行中</> : failed ? '包含失败或中断' : null}</span>
      <ChevronRight className="session-tool-group-chevron" size={13} aria-hidden="true" />
    </button>
    <div id={contentId} className="acp-process-group-body" hidden={!expanded}>{expanded && groups.map(renderGroup)}</div>
  </div>;
}

export function ProcessTimeline({ process, running, permissionTools, renderGroup }: {
  process: MessageGroup[]; running: boolean; permissionTools: Set<string>; renderGroup: (group: MessageGroup) => ReactNode;
}) {
  return <ol className="session-process-timeline">
    {groupProcess(process).map(item => {
      const Icon = item.kind === 'tools' ? Wrench : item.groups[0].entries.every(entry => entry.kind === 'thought') ? Brain : MessageSquare;
      return <li key={item.id} className={`session-process-item ${item.kind === 'tools' ? 'tools' : 'commentary'}`}>
        <span className="session-process-marker" aria-hidden="true"><Icon size={12} /></span>
        {item.kind === 'tools' ? <ToolGroup groups={item.groups} running={running} permissionTools={permissionTools} renderGroup={renderGroup} />
          : <div className="acp-process-message">{item.groups.map(renderGroup)}</div>}
      </li>;
    })}
  </ol>;
}
