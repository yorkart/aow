import { Folder, FolderOpen } from 'lucide-react';
import theme from '../../assets/seti/vs-seti-icon-theme.json';
import languages from '../../assets/seti/language-associations.json';
import './file-type-icon.css';

interface Props { path: string; className?: string }
interface DirectoryProps { expanded?: boolean; className?: string }

const fileNames = new Map(Object.entries(theme.fileNames));
const fileExtensions = new Map(Object.entries(theme.fileExtensions));
const languageNames = new Map(Object.entries(languages.fileNames));
const languageExtensions = new Map(Object.entries(languages.fileExtensions));
const languagePatterns = Object.entries(languages.filePatterns)
  .sort(([a], [b]) => b.length - a.length)
  .map(([pattern, icon]) => ({
    pattern: new RegExp(`^${pattern.split('*').map(part => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('.*')}$`),
    icon,
  }));
const icons = new Map(Object.entries(theme.iconDefinitions).map(([id, icon]) => [id, {
  character: String.fromCodePoint(parseInt(icon.fontCharacter.slice(1), 16)),
  color: 'fontColor' in icon ? icon.fontColor : 'var(--aow-muted)',
}]));

function iconForPath(path: string) {
  const name = path.split(/[/\\]/).pop()?.toLowerCase() ?? '';
  const named = fileNames.get(name);
  if (named) return named;

  // VS Code gives named files and compound extensions (e.g. test.tsx)
  // precedence over the language's icon. Only inspect the basename.
  const suffixes: string[] = [];
  for (let dot = name.indexOf('.'); dot >= 0; dot = name.indexOf('.', dot + 1)) {
    const suffix = name.slice(dot + 1);
    const icon = fileExtensions.get(suffix);
    if (icon) return icon;
    suffixes.push(suffix);
  }
  const language = languageNames.get(name) ?? languagePatterns.find(({ pattern }) => pattern.test(name))?.icon;
  if (language) return language;
  for (const suffix of suffixes) {
    const icon = languageExtensions.get(suffix);
    if (icon) return icon;
  }
  return theme.file;
}

export function DirectoryTypeIcon({ expanded = false, className = '' }: DirectoryProps) {
  const Icon = expanded ? FolderOpen : Folder;
  return <Icon aria-hidden="true" className={`directory-type-icon ${className}`.trim()} />;
}

export function FileTypeIcon({ path, className = '' }: Props) {
  const icon = icons.get(iconForPath(path)) ?? icons.get(theme.file)!;
  return <i aria-hidden="true" className={`file-type-icon ${className}`.trim()}
    data-icon={icon.character} style={{ color: icon.color }} />;
}
