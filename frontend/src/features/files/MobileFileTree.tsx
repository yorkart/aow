import { t } from '../../lib/i18n';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { filesApi } from './api';
import { DirectoryTypeIcon, FileTypeIcon } from './FileTypeIcon';
import { MobileState } from '../../mobile/MobilePrimitives';
import { useMobileResource } from '../../mobile/mobileState';

interface Props {
  directory: string; expanded: ReadonlySet<string>; refresh: number; visible: boolean; depth?: number;
  onToggle: (path: string) => void; onOpen: (path: string) => void;
}

export function MobileFileTree({ directory, expanded, refresh, visible, depth = 0, onToggle, onOpen }: Props) {
  const listing = useMobileResource(`file-tree.${directory}.${refresh}`, () => filesApi.listDirectory(directory), visible);
  return <div className="mobile-file-tree">
    <div className={depth ? 'mobile-file-tree-state' : undefined}>
      <MobileState loading={listing.loading && !listing.data} error={listing.error} retry={listing.reload}
        empty={listing.data?.entries.length === 0 ? t("files.this_directory_is_empty") : undefined} />
    </div>
    <ul>{listing.data?.entries.slice().sort((a, b) => Number(b.kind === 'directory') - Number(a.kind === 'directory') || a.name.localeCompare(b.name))
      .map(entry => {
        const folder = entry.kind === 'directory';
        const open = folder && expanded.has(entry.path);
        return <li key={entry.path}>
          <button className="mobile-file-row" style={{ paddingLeft: 12 + Math.min(depth, 6) * 18 }}
            aria-label={folder ? `${entry.name} ${t("files.folder")}` : entry.name} aria-expanded={folder ? open : undefined}
            title={entry.path} onClick={() => folder ? onToggle(entry.path) : onOpen(entry.path)}>
            {folder ? open ? <ChevronDown size={16} /> : <ChevronRight size={16} /> : <span className="mobile-file-chevron" />}
            {folder ? <DirectoryTypeIcon expanded={open} /> : <FileTypeIcon path={entry.path} />}
            <span>{entry.name}</span>
          </button>
          {open && <MobileFileTree directory={entry.path} expanded={expanded} refresh={refresh} visible={visible} depth={depth + 1} onToggle={onToggle} onOpen={onOpen} />}
        </li>;
      })}</ul>
  </div>;
}
