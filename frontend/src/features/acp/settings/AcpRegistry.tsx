import { useState, type ReactNode } from 'react';
import { Bot, FileText, Github, Link, Search } from 'lucide-react';
import type { AgentInfo } from '../types';

function webUrl(value?: string | null) {
  if (!value) return undefined;
  try { const url = new URL(value); return ['https:', 'http:'].includes(url.protocol) ? url.href : undefined; } catch { return undefined; }
}

function AgentIcon({ agent }: { agent: AgentInfo }) {
  const [failed, setFailed] = useState(false);
  const url = webUrl(agent.icon);
  return url && !failed ? <img src={url} alt="" loading="lazy" referrerPolicy="no-referrer" onError={() => setFailed(true)} /> : <Bot aria-hidden="true" />;
}

export function AcpRegistry({ registry, agents, installButton }: {
  registry: AgentInfo[]; agents: AgentInfo[]; installButton: (agent: AgentInfo, registryEntry: boolean) => ReactNode;
}) {
  const [search, setSearch] = useState('');
  const [filter, setFilter] = useState('All');
  const installed = new Set(agents.filter(agent => agent.installed).map(agent => agent.id));
  const query = search.trim().toLowerCase();
  const visible = registry.filter(agent => `${agent.id} ${agent.name} ${agent.description}`.toLowerCase().includes(query)
    && (filter === 'All' || (filter === 'Installed') === installed.has(agent.id)));
  return <section className="acp-registry-browser" aria-label="ACP Registry">
    <div className="acp-registry-toolbar">
      <label className="acp-registry-search"><Search aria-hidden="true" /><input type="search" aria-label="搜索 ACP Registry" placeholder="Search agents..." value={search} onChange={event => setSearch(event.target.value)} /></label>
      <div className="acp-registry-filters" role="group" aria-label="安装状态筛选">{['All', 'Installed', 'Not Installed'].map(value => <button key={value} type="button" aria-pressed={filter === value} onClick={() => setFilter(value)}>{value}</button>)}</div>
    </div>
    <div className="acp-registry-cards">{visible.map(agent => <article key={agent.id} className="acp-registry-card">
      <div className="acp-registry-card-heading"><span className="acp-registry-icon"><AgentIcon agent={agent} /></span><strong>{agent.name}</strong>{agent.version && <span className="acp-registry-version">v{agent.version.replace(/^v/, '')}</span>}<div className="acp-registry-install">{installButton(agent, true)}</div></div>
      <div className="acp-registry-card-details"><p title={agent.description}>{agent.description}</p><span className="acp-registry-id" title={`ID: ${agent.id}`}>ID: {agent.id}</span><div className="acp-registry-links">{[
        { value: agent.repository, label: 'Repository', Icon: Github },
        { value: agent.website, label: 'Website', Icon: Link },
        { value: agent.license_url, label: 'License', Icon: FileText },
      ].map(({ value, label, Icon }) => { const url = webUrl(value); return url ? <a key={label} href={url} target="_blank" rel="noopener noreferrer" aria-label={`${agent.name} ${label}`} title={label}><Icon aria-hidden="true" /></a> : null; })}</div></div>
    </article>)}</div>
    {!visible.length && <p className="project-aow-empty" role="status">没有匹配的 Agent。</p>}
  </section>;
}
