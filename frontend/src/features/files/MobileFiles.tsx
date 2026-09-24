import { ChevronRight, File, FileText, Folder, FolderOpen } from 'lucide-react';
import { filesApi } from './api';
import { MarkdownContent } from '../../components/MarkdownContent';
import { MobilePageHeader, MobileRefresh, MobileState } from '../../mobile/MobilePrimitives';
import { useMobileResource, useMobileScroll, type MobileRoute } from '../../mobile/mobileState';
import type { MobileNavigate } from '../../mobile/mobileState';

const basename = (path: string) => path.split('/').filter(Boolean).pop() ?? path;
function within(root: string, path: string) { return path === root || path.startsWith(`${root.replace(/\/$/, '')}/`); }

export function MobileFiles({ route, notesPath, visible, navigate, back }: {
  route: MobileRoute; notesPath: string; visible: boolean; navigate: MobileNavigate; back: () => void;
}) {
  const root = route.notes === '1' ? notesPath : route.workspace;
  const directory = route.directory && (route.fileSource === 'external' || within(root, route.directory)) ? route.directory : root;
  const listing = useMobileResource(`directory.${directory}`, () => filesApi.listDirectory(directory), visible && !route.file);
  const scroll = useMobileScroll(`directory.${directory}`, !!listing.data);
  const base: Partial<MobileRoute> = { workspace: route.workspace, view: 'files', notes: route.notes, fileSource: route.fileSource };
  if (route.file) return <MobileFileReader path={route.file} back={back} />;
  return <section className="mobile-content-page">
    <MobilePageHeader title={directory === root ? (route.notes === '1' ? 'Notes' : '文件') : basename(directory)}
      subtitle={directory === root ? '浏览工作区内容' : directory.slice(root.length + 1)} back={directory === root ? undefined : back}
      actions={<MobileRefresh reload={listing.reload} loading={listing.loading} />} />
    <div className="mobile-segments" role="tablist" aria-label="文件来源">
      <button role="tab" aria-selected={route.notes !== '1'} onClick={() => navigate({ workspace: route.workspace, view: 'files' }, true)}><Folder size={16} />项目文件</button>
      <button role="tab" aria-selected={route.notes === '1'} onClick={() => navigate({ workspace: route.workspace, view: 'files', notes: '1' }, true)}><FileText size={16} />Notes</button>
    </div>
    <div className="mobile-scroll" ref={scroll}>
      <MobileState loading={listing.loading && !listing.data} error={listing.error} retry={listing.reload} empty={listing.data?.entries.length === 0 ? '这个目录是空的。' : undefined} />
      <div className="mobile-list">{listing.data?.entries.slice().sort((a, b) => Number(b.kind === 'directory') - Number(a.kind === 'directory') || a.name.localeCompare(b.name))
        .map((entry) => <button key={entry.path} className="mobile-list-row" onClick={() => navigate({ ...base, ...(entry.kind === 'directory' ? { directory: entry.path } : { directory, file: entry.path }) })}>
          <div className={`mobile-row-icon ${entry.kind === 'directory' ? 'folder' : ''}`}>{entry.kind === 'directory' ? <FolderOpen size={21} /> : <File size={20} />}</div>
          <div className="mobile-row-main"><strong>{entry.name}</strong><span>{entry.kind === 'directory' ? '文件夹' : `${Math.max(1, Math.ceil(entry.size / 1024))} KB`}</span></div><ChevronRight size={17} />
        </button>)}</div>
    </div>
  </section>;
}

function MobileFileReader({ path, back }: { path: string; back: () => void }) {
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
    <MobilePageHeader title={basename(path)} subtitle="文件预览" back={back} actions={<MobileRefresh reload={data.reload} loading={data.loading} />} />
    <div className="mobile-scroll mobile-file-reader" ref={scroll}>
      <MobileState loading={data.loading && !data.data} error={data.error} retry={data.reload} />
      {image && <img className="mobile-file-image" src={filesApi.rawUrl(path)} alt={basename(path)} />}
      {pdf && <><a className="mobile-button" href={filesApi.rawUrl(path)}>打开 PDF</a><iframe className="mobile-pdf" title={basename(path)} src={filesApi.rawUrl(path)} /></>}
      {data.data && !image && !pdf && (data.data.html ? <div className="mobile-markdown" dangerouslySetInnerHTML={{ __html: data.data.content }} />
        : markdown ? <MarkdownContent text={data.data.content} className="mobile-markdown" /> : <pre className="mobile-code">{data.data.content}</pre>)}
    </div>
  </section>;
}
