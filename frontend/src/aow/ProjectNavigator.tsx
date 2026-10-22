import { useId, useRef, useState, type ReactNode } from 'react';
import { ChevronDown } from 'lucide-react';
import { RowMenu } from '../components/RowMenu';
import { t } from '../lib/i18n';
import type { AowProject } from './types';

export function ProjectNavigator({ projects, projectId, actions }: {
  projects: AowProject[]; projectId: (id: string) => string; actions: ReactNode;
}) {
  const id = useId();
  const trigger = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState(false);
  const title = t('navigation.projects');
  const locate = (project: AowProject) => {
    setOpen(false);
    const section = document.getElementById(projectId(project.id));
    const header = section?.querySelector<HTMLElement>('.aow-panel-header');
    const viewport = section?.closest<HTMLElement>('.aow-panel-viewport');
    const button = header?.querySelector<HTMLButtonElement>('.aow-panel-header-title');
    if (!header || !viewport || !button) { trigger.current?.focus({ preventScroll: true }); return; }
    viewport.scrollTo({ top: viewport.scrollTop + header.getBoundingClientRect().top - viewport.getBoundingClientRect().top - (trigger.current?.closest('header')?.offsetHeight ?? 0),
      behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' });
    button.focus({ preventScroll: true });
    if (!window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      header.animate([{ backgroundColor: 'var(--aow-subtle-hover)' }, { backgroundColor: 'var(--aow-panel)' }], { duration: 1400 });
    }
  };
  return <header className="side-title aow-panel-header project-aow-section-title project-aow-project-list-title project-navigator">
    <button ref={trigger} type="button" className="aow-panel-header-title project-navigator-trigger" aria-haspopup="menu"
      aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(value => !value)}
      onKeyDown={event => { if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setOpen(true); } }}>
      <strong>{title}</strong><ChevronDown className={open ? 'project-navigator-open' : undefined} aria-hidden="true" />
    </button>
    <span className="aow-panel-header-actions">{actions}</span>
    {open && <RowMenu id={id} label={title} anchor={trigger} onClose={() => setOpen(false)}>
      {projects.map(project => <button key={project.id} type="button" role="menuitem" title={project.registered_path} onClick={() => locate(project)}>
        <span>{project.name}<small className="project-navigator-path">{project.registered_path}</small></span>
      </button>)}
      {!projects.length && <p>{t('aow.no_projects')}</p>}
    </RowMenu>}
  </header>;
}
