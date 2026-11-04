import { ChevronRight } from 'lucide-react';
import { FileTypeIcon } from '../files/FileTypeIcon';
import { t } from '../../lib/i18n';

export function MobileGitFileRow({ path, originalPath, status, onOpen }: {
  path: string; originalPath?: string | null; status: string; onOpen: () => void;
}) {
  return <button className="mobile-source-file" onClick={onOpen} title={path}>
    <span className="mobile-source-status" data-status={status[0]} title={status === '?' ? t("git.untracked") : status}>{status === '?' ? 'U' : status[0]}</span>
    <FileTypeIcon path={path} />
    <span className="mobile-source-file-path"><span>{path}</span>{originalPath && <small>{originalPath} → {path}</small>}</span>
    <ChevronRight size={16} />
  </button>;
}
