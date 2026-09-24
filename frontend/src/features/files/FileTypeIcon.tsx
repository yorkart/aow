import {
  Archive, Braces, CodeXml, Database, File, FileArchive, FileCode2, FileCog,
  FileImage, FileJson, FileSpreadsheet, FileText, Folder, FolderOpen, Image,
  Palette, ScrollText, TerminalSquare,
} from 'lucide-react';
import { extension } from '../editor/language';

interface Props { path: string; className?: string }
interface DirectoryProps { expanded?: boolean; className?: string }

export function DirectoryTypeIcon({ expanded = false, className = '' }: DirectoryProps) {
  const Icon = expanded ? FolderOpen : Folder;
  return <Icon className={`directory-type-icon ${className}`.trim()} />;
}

export function FileTypeIcon({ path, className = '' }: Props) {
  const name = path.split('/').pop()?.toLowerCase() ?? path.toLowerCase();
  const ext = extension(path);
  const classes = (kind: string) => `file-type-icon ${kind} ${className}`.trim();

  if (['png', 'jpg', 'jpeg', 'gif', 'webp', 'svg', 'bmp', 'ico', 'avif'].includes(ext)) return <FileImage className={classes('image')} />;
  if (ext === 'json' || ['package-lock.json', 'tsconfig.json'].includes(name)) return <FileJson className={classes('json')} />;
  if (['md', 'markdown', 'txt', 'log', 'rst'].includes(ext) || ['readme', 'license', 'notice'].includes(name)) return <FileText className={classes('text')} />;
  if (['html', 'htm', 'xml', 'vue', 'svelte', 'astro'].includes(ext)) return <CodeXml className={classes('markup')} />;
  if (['css', 'scss', 'sass', 'less', 'styl', 'stylus', 'postcss'].includes(ext)) return <Palette className={classes('stylesheet')} />;
  if (['rs', 'ts', 'tsx', 'js', 'jsx', 'py', 'go', 'java', 'c', 'cc', 'cpp', 'h', 'hpp', 'cs', 'php', 'rb', 'swift', 'kt'].includes(ext)) return <FileCode2 className={classes('code')} />;
  if (['sh', 'bash', 'zsh', 'fish', 'ps1', 'bat', 'cmd'].includes(ext)) return <TerminalSquare className={classes('shell')} />;
  if (['toml', 'yaml', 'yml', 'ini', 'conf', 'config', 'properties', 'env'].includes(ext) || name.startsWith('.')) return <FileCog className={classes('config')} />;
  if (['zip', 'gz', 'tgz', 'tar', 'bz2', 'xz', '7z', 'rar'].includes(ext)) return <FileArchive className={classes('archive')} />;
  if (['csv', 'xls', 'xlsx'].includes(ext)) return <FileSpreadsheet className={classes('sheet')} />;
  if (['sql', 'db', 'sqlite', 'sqlite3'].includes(ext)) return <Database className={classes('database')} />;
  if (['pdf', 'doc', 'docx', 'odt'].includes(ext)) return <ScrollText className={classes('document')} />;
  if (['lock'].includes(ext)) return <Braces className={classes('lock')} />;
  if (['wasm', 'bin', 'exe'].includes(ext)) return <Archive className={classes('binary')} />;
  if (['image'].includes(ext)) return <Image className={classes('image')} />;
  return <File className={classes('default')} />;
}
