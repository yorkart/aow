import { useState } from 'react';
import { ArrowUp, Bot, Folder, FolderGit2, GitBranch, Sparkles } from 'lucide-react';
import { Avatar } from './ui';
import { taskDirectory, type Agent, type Org, type Project, type Task } from './model';

export function TaskComposer({ org, projects, agents, nextId, initialProject, onCreate, onAddAgent }: {
  org: Org; projects: Project[]; agents: Agent[]; nextId: string; initialProject: string;
  onCreate: (task: Task) => void; onAddAgent: () => void;
}) {
  const [prompt, setPrompt] = useState('');
  const [projectId, setProjectId] = useState(initialProject);
  const [agentId, setAgentId] = useState('');
  const [worktree, setWorktree] = useState(true);
  const project = projects.find(item => item.id === projectId);
  const agent = agents.find(item => item.id === agentId);
  const cwd = taskDirectory(org, nextId, project, worktree);
  const canCreate = Boolean(prompt.trim() && agent);
  return <div className="ow-new-task">
    <div className="ow-new-task-heading"><span className="ow-star"><Sparkles size={25} /></span><p>{org.name} / NEW TASK</p><h1>下一件想完成的事？</h1><span>选一位 agent，从一个想法开始。</span></div>
    <form className="ow-composer" onSubmit={event => {
      event.preventDefault();
      if (!canCreate) return;
      onCreate({ id: nextId, orgId: org.id, title: prompt.trim().split('\n')[0].slice(0, 55), prompt: prompt.trim(), projectId, agentId, worktree: Boolean(project?.repository && worktree), cwd, status: 'running', progress: 0, time: '刚刚', messages: [] });
    }}>
      <label className="ow-sr-only" htmlFor="ow-task-prompt">任务描述</label>
      <textarea id="ow-task-prompt" value={prompt} onChange={event => setPrompt(event.target.value)} placeholder="描述你的任务，agent 会立即开始工作…" autoFocus required />
      <div className="ow-composer-options">
        <label><Folder size={15} /><select aria-label="关联项目" value={projectId} onChange={event => setProjectId(event.target.value)}><option value="">不关联项目</option>{projects.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}</select></label>
        <label>{agent ? <Avatar agent={agent} small /> : <Bot size={15} />}<select aria-label="执行 Agent" value={agentId} required onChange={event => setAgentId(event.target.value)}><option value="" disabled>选择 Agent · 必选</option>{agents.map(item => <option key={item.id} value={item.id}>{item.name} · {item.role}</option>)}</select></label>
        <button className="ow-send" type="submit" disabled={!canCreate} aria-label="创建并执行任务" title="创建并执行任务"><ArrowUp size={19} /></button>
      </div>
      {project?.repository && <fieldset className="ow-worktree-choice"><legend>执行环境</legend>
        <label><input type="radio" name="workspace" checked={worktree} onChange={() => setWorktree(true)} /><GitBranch size={15} /><span>创建独立 worktree<small>在仓库的同级目录中隔离改动</small></span></label>
        <label><input type="radio" name="workspace" checked={!worktree} onChange={() => setWorktree(false)} /><FolderGit2 size={15} /><span>使用项目仓库<small>直接在现有目录工作</small></span></label>
      </fieldset>}
    </form>
    <div className="ow-cwd-preview"><Folder size={14} /><span>工作目录<code>{cwd}</code></span><span className="ow-small-label">{project?.repository ? worktree ? '独立 worktree' : '项目仓库' : '自动分配'}</span></div>
    {!agents.length && <p className="ow-help">这个组织还没有 agent。<button className="ow-text-button" onClick={onAddAgent}>配置第一个 Agent</button></p>}
    <div className="ow-suggestions"><span>也可以从这里开始</span>{['梳理项目结构，提出改进建议', '设计一个更清晰的任务创建流程', '整理今天的想法与待办事项'].map(text => <button key={text} onClick={() => setPrompt(text)}>{text}<ArrowUp size={13} /></button>)}</div>
    <p className="ow-demo-note">设计预览 · 任务运行和文件内容均为演示数据</p>
  </div>;
}
