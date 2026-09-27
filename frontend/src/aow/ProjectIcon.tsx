import { useEffect, useState } from 'react';
import { FolderGit2 } from 'lucide-react';
import { aowApi } from './aowApi';
import type { AowProject } from './types';
import './project-icon.css';

function imageUrl(value: string | null | undefined) {
  if (!value) return undefined;
  try {
    const url = new URL(value);
    return ['http:', 'https:'].includes(url.protocol) && !url.username && !url.password ? value : undefined;
  } catch { return undefined; }
}

export function ProjectIcon({ project, size = 14 }: { project: AowProject; size?: number }) {
  const [revision, setRevision] = useState(0);
  const [resolved, setResolved] = useState<{ key: string; url: string | null }>();
  const [failed, setFailed] = useState<string>();
  const key = JSON.stringify([project.id, project.registered_path, project.avatar_url, revision]);
  useEffect(() => {
    const refresh = () => setRevision(value => value + 1);
    window.addEventListener('aow-review-providers-changed', refresh);
    return () => window.removeEventListener('aow-review-providers-changed', refresh);
  }, []);
  useEffect(() => {
    if (project.builtin) return;
    let live = true;
    const controller = new AbortController();
    const timer = window.setTimeout(() => controller.abort(), 15_000);
    void aowApi.projectAvatar(project.id, controller.signal)
      .then(value => { if (live) setResolved({ key, url: value.avatar_url }); })
      .catch(() => { /* Keep the saved avatar when metadata is unavailable. */ })
      .finally(() => window.clearTimeout(timer));
    return () => { live = false; controller.abort(); window.clearTimeout(timer); };
  }, [key, project.id, project.builtin]);
  const url = project.builtin ? undefined : imageUrl(resolved?.key === key ? resolved.url : project.avatar_url);
  const imageKey = `${key}:${url}`;
  const style = { width: size, height: size, flexBasis: size };
  return url && failed !== imageKey
    ? <img key={imageKey} className="project-icon" src={url} alt="" aria-hidden="true" draggable={false}
      referrerPolicy="no-referrer" decoding="async" style={style} onError={() => setFailed(imageKey)} />
    : <FolderGit2 className="project-icon" aria-hidden="true" size={size} style={style} />;
}
