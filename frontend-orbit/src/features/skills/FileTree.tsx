import { useState } from 'react';
import { ChevronDown, ChevronRight, File, Folder, Image } from 'lucide-react';
import { mime } from './files';
import type { SkillEntry } from './types';

export function FileTree({ files, selected, onSelect }: { files: SkillEntry[]; selected: string; onSelect: (path: string) => void }) {
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const rows: { entry: SkillEntry; depth: number }[] = [];
  const visit = (parent: string, depth: number) => {
    const children = files.filter(file => (file.path.includes('/') ? file.path.slice(0, file.path.lastIndexOf('/')) : '') === parent)
      .sort((a, b) => a.path === 'SKILL.md' ? -1 : b.path === 'SKILL.md' ? 1 : a.kind === b.kind ? a.path.localeCompare(b.path) : a.kind === 'directory' ? -1 : 1);
    for (const entry of children) { rows.push({ entry, depth }); if (entry.kind === 'directory' && !collapsed.has(entry.path)) visit(entry.path, depth + 1); }
  };
  visit('', 0);
  return <div className="skills-file-tree" role="group" aria-label="技能文件">{rows.map(({ entry, depth }) => {
    const Icon = entry.kind === 'directory' ? Folder : mime(entry.path).startsWith('image/') ? Image : File;
    return <button key={entry.path} type="button" aria-label={entry.path} aria-pressed={selected === entry.path} style={{ paddingLeft: 10 + depth * 14 }} onClick={() => {
      onSelect(entry.path);
      if (entry.kind === 'directory') setCollapsed(current => { const next = new Set(current); if (next.has(entry.path)) next.delete(entry.path); else next.add(entry.path); return next; });
    }}>{entry.kind === 'directory' ? collapsed.has(entry.path) ? <ChevronRight size={12} /> : <ChevronDown size={12} /> : <span className="skills-tree-spacer" />}<Icon size={14} /><span>{entry.path.split('/').pop()}</span></button>;
  })}</div>;
}
