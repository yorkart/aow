import { useCallback, useEffect, useRef, useState } from 'react';
import { Boxes, Folder, Laptop, Layers3, ListTodo, Unplug, Users, X, Zap } from 'lucide-react';
import { AuthScreen } from './features/auth/AuthScreen';
import { OrgForm } from './OrgForm';
import { OrgSwitcher } from './OrgSwitcher';
import { createOrg, loadOrgs, selectOrg, type OrgState } from './orgs';
import { SkillsPage } from './features/skills/SkillsPage';
import { ConnectorsPage } from './features/connectors/ConnectorsPage';

const navigation = [
  { id: 'tasks', label: 'Tasks', icon: ListTodo },
  { id: 'projects', label: 'Projects', icon: Folder },
  { id: 'automation', label: 'Automation', icon: Zap },
  { id: 'agents', label: 'Agents', icon: Users },
  { id: 'skills', label: 'Skills', icon: Boxes },
  { id: 'connectors', label: 'Connectors', icon: Unplug },
] as const;

function CreateOrgDialog({ onClose, onCreate }: { onClose: () => void; onCreate: (name: string) => Promise<void> }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => { const dialog = ref.current!; dialog.showModal(); return () => dialog.close(); }, []);
  return <dialog className="orbit-dialog" aria-labelledby="orbit-org-title" ref={ref} onCancel={onClose}>
    <button className="orbit-dialog-close" type="button" aria-label="关闭" onClick={onClose}><X size={16} aria-hidden="true" /></button>
    <OrgForm onCreate={onCreate} />
  </dialog>;
}

export function App() {
  const [state, setState] = useState<OrgState>();
  const [error, setError] = useState('');
  const [retry, setRetry] = useState(0);
  const [section, setSection] = useState<(typeof navigation)[number]['id']>('tasks');
  const [creating, setCreating] = useState(false);
  const [skillsHome, setSkillsHome] = useState(0);
  const [connectorsHome, setConnectorsHome] = useState(0);
  const editorDirty = useRef(false);
  const onEditorDirty = useCallback((dirty: boolean) => { editorDirty.current = dirty; }, []);
  const canLeave = () => !editorDirty.current || window.confirm('放弃尚未保存的修改？');
  useEffect(() => {
    const controller = new AbortController();
    void loadOrgs(controller.signal).then(next => { if (!controller.signal.aborted) setState(next); }).catch((reason: unknown) => {
      if (!controller.signal.aborted) setError(reason instanceof Error ? reason.message : '无法读取组织。');
    });
    return () => controller.abort();
  }, [retry]);
  const create = async (name: string) => { setState(await createOrg(name)); setCreating(false); setSection('tasks'); };
  if (error) return <AuthScreen title="无法读取工作空间" detail={error} retry={() => { setError(''); setRetry(value => value + 1); }} />;
  if (!state) return <AuthScreen title="正在打开工作空间…" />;
  if (!state.orgs.length) return <main className="orbit-onboarding"><section className="orbit-onboarding-card" aria-labelledby="orbit-org-title"><OrgForm initial onCreate={create} /></section></main>;
  const current = state.orgs.find(org => org.id === state.active_org_id) ?? state.orgs[0];
  const active = navigation.find(item => item.id === section)!;
  return <div className="orbit-workspace">
    <nav className="orbit-rail" aria-label="功能导航">
      <div className="orbit-brand" title="AoW Orbit"><Layers3 size={24} aria-hidden="true" /></div>
      {navigation.map(item => <button key={item.id} type="button" aria-label={item.label} title={item.label} aria-current={section === item.id ? 'page' : undefined} onClick={() => { if (section === item.id || canLeave()) setSection(item.id); }}><item.icon size={21} aria-hidden="true" /></button>)}
    </nav>
    <aside className="orbit-sidebar" aria-label="左侧面板">
      <header className="orbit-sidebar-header"><OrgSwitcher current={current} orgs={state.orgs} onSelect={async id => { if (!canLeave()) return; setState(await selectOrg(id)); setSection('tasks'); }} onCreate={() => { if (canLeave()) setCreating(true); }} /></header>
      <div key={`${current.id}:${section}`} className="orbit-sidebar-body" aria-label={`${active.label} 列表`}>{section === 'skills' && <div className="skills-sidebar"><h2>SKILLS</h2><button type="button" aria-current="page" onClick={() => { if (canLeave()) setSkillsHome(value => value + 1); }}><Boxes size={16} />Installed</button></div>}{section === 'connectors' && <div className="connectors-sidebar"><h2>CONNECTORS</h2><button type="button" aria-current="page" onClick={() => { if (canLeave()) setConnectorsHome(value => value + 1); }}><Laptop size={16} />本地 Connector</button></div>}</div>
    </aside>
    <main key={current.id} className="orbit-content" aria-label={`${active.label} 工作区`}>{section === 'skills' && <SkillsPage key={`${current.id}:${skillsHome}`} org={current.id} onDirtyChange={onEditorDirty} />}{section === 'connectors' && <ConnectorsPage key={`${current.id}:${connectorsHome}`} org={current.id} onDirtyChange={onEditorDirty} />}</main>
    {creating && <CreateOrgDialog onClose={() => setCreating(false)} onCreate={create} />}
  </div>;
}
