import { useEffect, useId, useRef, useState, type FormEvent } from 'react';
import { createPortal } from 'react-dom';
import { LoaderCircle, Play, X } from 'lucide-react';
import type { AowProject } from '../../aow/types';
import type { AowAgent } from '../agents/types';
import type { InboxExecuteOptions, InboxItem } from './types';
import { WorkspaceSelect } from '../workspaces/WorkspaceSelect';
import { workspaceConfig, type WorkspaceConfig } from '../workspaces/types';

const interactiveAgent = (agent: AowAgent) => ['codex', 'traecli', 'hermes'].includes(agent.agent_type ?? agent.id);

export function InboxExecutePanel({ item, project, agents, busy, error, container, onClose, onSubmit }: {
  item: InboxItem;
  project: AowProject;
  agents: AowAgent[];
  busy: boolean;
  error?: string;
  container: HTMLElement;
  onClose: () => void;
  onSubmit: (options: InboxExecuteOptions) => void;
}) {
  const worktrees = [...project.worktrees].sort((left, right) => Number(right.is_main) - Number(left.is_main));
  const mainWorktree = worktrees[0];
  const availableAgents = agents.filter(agent => agent.available && interactiveAgent(agent));
  const [agent, setAgent] = useState(() => availableAgents[0]?.id ?? '');
  const [workspace, setWorkspace] = useState<WorkspaceConfig>({ workspace_mode: 'new_worktree', workspace_path: mainWorktree?.path ?? '', base_branch: '' });
  const [workspaceReady, setWorkspaceReady] = useState(false);
  const [appendPrompt, setAppendPrompt] = useState('');
  const dialog = useRef<HTMLElement>(null);
  const titleId = useId();

  useEffect(() => {
    const element = dialog.current;
    element?.querySelector<HTMLElement>('[aria-label="Agent"]')?.focus({ preventScroll: true });
  }, []);

  const finalPrompt = appendPrompt ? `${item.markdown}\n\n${appendPrompt}` : item.markdown;

  const onDialogKeyDown = (event: React.KeyboardEvent<HTMLElement>) => {
    event.stopPropagation();
    if (event.key === 'Escape') {
      event.preventDefault();
      if (!busy) onClose();
      return;
    }
    if (event.key !== 'Tab') return;
    const focusable = [...(dialog.current?.querySelectorAll<HTMLElement>(
      'button:not(:disabled),select:not(:disabled),input:not(:disabled),textarea:not(:disabled),summary,[tabindex="0"]',
    ) ?? [])];
    if (!focusable.length) return;
    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && (document.activeElement === first || !dialog.current?.contains(document.activeElement))) {
      event.preventDefault(); last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault(); first.focus();
    }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (busy || !agent || !workspaceReady) return;
    onSubmit({ agent, ...workspaceConfig(workspace), append_prompt: appendPrompt });
  };


  return createPortal(<div className="inbox-execute-overlay" onPointerDown={event => {
    if (event.target === event.currentTarget && !busy) onClose();
  }}>
    <section ref={dialog} className="inbox-execute-panel" role="dialog" aria-modal="true" aria-labelledby={titleId} onKeyDown={onDialogKeyDown}>
    <form onSubmit={submit}>
      <header><div><Play size={15} /><div><h2 id={titleId}>执行需求</h2><p title={item.markdown}>{item.markdown.split(/\r?\n/, 1)[0]}</p></div></div><button type="button" aria-label="关闭" disabled={busy} onClick={onClose}><X size={16} /></button></header>
      <div className="inbox-execute-body">
        <label><span>Agent</span><select autoFocus aria-label="Agent" required value={agent} onChange={event => setAgent(event.target.value)} disabled={busy}>
          {!availableAgents.length && <option value="">没有可用的交互式 Agent</option>}
          {agents.map(option => <option key={option.id} value={option.id} disabled={!option.available || !interactiveAgent(option)}>{option.display_name}{!option.available ? ' · 未安装' : !interactiveAgent(option) ? ' · 暂不支持交互执行' : ''}</option>)}
        </select></label>
        <WorkspaceSelect project={project} value={workspace} disabled={busy} onChange={setWorkspace} onValidityChange={setWorkspaceReady} />
        {workspace.workspace_mode === 'temporary' && <p className="workspace-hint">此工作区会保留供 Agent 使用，不会自动清理。</p>}
        <label className="inbox-append-prompt"><span>追加 prompt <small>可选</small></span><textarea aria-label="追加 prompt" rows={5} value={appendPrompt} onChange={event => setAppendPrompt(event.target.value)} placeholder="例如：先分析需求和实现方案，列出风险，再开始修改代码。" disabled={busy} /></label>
        <details className="inbox-final-prompt"><summary>查看最终任务输入</summary><pre>{finalPrompt}</pre></details>
        {error && <p className="inbox-execute-error" role="alert">{error}</p>}
      </div>
      <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button className="inbox-execute-primary" type="submit" disabled={busy || !agent || !workspaceReady}>{busy ? <LoaderCircle className="inbox-spin" size={14} /> : <Play size={14} />}{busy ? '正在启动…' : '开始执行'}</button></footer>
    </form>
    </section>
  </div>, container);
}
