import { t } from '../../lib/i18n';
import { useState } from 'react';
import { ArrowLeft } from 'lucide-react';
import { filesApi } from './api';
import { MarkdownFileContent } from './MarkdownFileContent';
import { MobileFileTree } from './MobileFileTree';
import { MobilePageHeader, MobilePanelTabs, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { saveMobileValue, useMobileResource, useMobileScroll, type MobileRoute, type MobileNavigate } from '../../mobile/mobileState';
import './mobile-files.css';

const basename = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
function within(root: string, path: string) { return !!root && (path === root || path.startsWith(`${root.replace(/\/$/, '')}/`)); }
const sources = [
  { id: 'projects', label: t("files.project_files") },
  { id: 'notes', label: t("files.notes_files") },
] as const;

export function MobileFiles({ route, workspace, notesPath, visible, navigate, back }: {
  route: MobileRoute; workspace: string; notesPath: string; visible: boolean; navigate: MobileNavigate; back: () => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set());
  const [refresh, setRefresh] = useState(0);
  const path = route.directory ?? route.file ?? '';
  const source = route.notes === '1' || route.fileSource === 'notes' || within(notesPath, path) ? 'notes' : 'projects';
  const root = source === 'notes' ? notesPath : route.workspace;
  const directory = route.directory && (route.fileSource === 'external' || within(root, route.directory)) ? route.directory : root;
  const scrollKey = `file-tree.${source}.${directory}`;
  const scroll = useMobileScroll(scrollKey, !!directory && !route.file);
  const base: Partial<MobileRoute> = { workspace: route.workspace, view: 'files', notes: source === 'notes' ? '1' : undefined,
    fileSource: source === 'notes' ? 'notes' : within(route.workspace, directory) ? 'project' : 'external' };
  const parent = directory.replace(/\/$/, '').slice(0, directory.replace(/\/$/, '').lastIndexOf('/')) || '/';
  return <><section className="mobile-content-page mobile-explorer" hidden={!!route.file}>
    <MobilePageHeader title={t("navigation.explorer")} actions={<MobileRefresh reload={() => setRefresh(value => value + 1)} />} />
    <MobilePanelTabs label={t("files.file_source")} items={sources} selected={source} onSelect={id => navigate({ workspace: route.workspace, view: 'files',
      notes: id === 'notes' ? '1' : undefined }, true)} />
    <div className="mobile-files-location">
      {directory !== root && <button className="mobile-icon-button" aria-label={t("mobile.back")} onClick={() => navigate({ ...base, directory: parent === root ? undefined : parent })}><ArrowLeft size={18} /></button>}
      <span title={directory}>{directory}</span>
    </div>
    <div className="mobile-scroll mobile-file-tree-scroll" ref={scroll}>
      <MobileFileTree directory={directory} expanded={expanded} refresh={refresh} visible={visible && !route.file}
        onToggle={path => setExpanded(current => { const next = new Set(current); if (!next.delete(path)) next.add(path); return next; })}
        onOpen={file => {
          saveMobileValue(`scroll.${scrollKey}`, String(scroll.current?.scrollTop ?? 0));
          navigate({ ...base, directory, file });
        }} />
    </div>
  </section>{route.file && <MobileFileReader path={route.file} workspace={workspace} back={back} />}</>;
}

function MobileFileReader({ path, workspace, back }: { path: string; workspace: string; back: () => void }) {
  const image = /\.(png|jpe?g|gif|webp|svg|avif|ico|bmp)$/i.test(path);
  const pdf = /\.pdf$/i.test(path);
  const markdown = /\.(md|markdown)$/i.test(path);
  const docx = /\.docx$/i.test(path);
  const data = useMobileResource(`file.${path}`, async () => {
    if (image || pdf) return { content: '', html: false };
    if (docx) {
      const [{ default: mammoth }, { default: DOMPurify }, arrayBuffer] = await Promise.all([import('mammoth'), import('dompurify'), filesApi.readBinary(path)]);
      const result = await mammoth.convertToHtml({ arrayBuffer });
      return { content: DOMPurify.sanitize(result.value), html: true };
    }
    return { content: (await filesApi.readText(path)).content, html: false };
  });
  const scroll = useMobileScroll(`file.${path}`, !!data.data);
  return <section className="mobile-content-page">
    <MobilePageHeader title={basename(path)} subtitle={t("files.file_preview")} back={back} actions={<MobileRefresh reload={data.reload} loading={data.loading} />} />
    <div className="mobile-scroll mobile-file-reader" ref={scroll}>
      <MobileState loading={data.loading && !data.data} error={data.error} retry={data.reload} />
      {image && <img className="mobile-file-image" src={filesApi.rawUrl(path)} alt={basename(path)} />}
      {pdf && <><a className="mobile-button" href={filesApi.rawUrl(path)}>{t("files.open_pdf")}</a><iframe className="mobile-pdf" title={basename(path)} src={filesApi.rawUrl(path)} /></>}
      {data.data && !image && !pdf && (data.data.html ? <div className="mobile-markdown" dangerouslySetInnerHTML={{ __html: data.data.content }} />
        : markdown ? <MarkdownFileContent text={data.data.content} className="mobile-markdown" workspace={workspace} path={path} /> : <pre className="mobile-code">{data.data.content}</pre>)}
    </div>
  </section>;
}
