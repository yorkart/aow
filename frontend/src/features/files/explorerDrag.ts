export const EXPLORER_PATH_MIME = 'application/x-aow-explorer-path';

export function writeExplorerPath(data: DataTransfer, path: string) {
  data.effectAllowed = 'copy';
  data.setData(EXPLORER_PATH_MIME, path);
  data.setData('text/plain', path);
}

export function readExplorerPath(data: DataTransfer): string | undefined {
  const path = data.getData(EXPLORER_PATH_MIME);
  // Control characters can become terminal keystrokes, even inside shell quotes.
  // eslint-disable-next-line no-control-regex -- Validate or match literal protocol control bytes intentionally.
  if (!path.startsWith('/') || /[\x00-\x1f\x7f-\x9f]/.test(path)) return undefined;
  return path;
}
