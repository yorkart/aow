import { t } from '../../lib/i18n';
export function EditorShortcutHint() {
  const shortcut = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent) ? '⌘ + Enter' : 'Ctrl + Enter';
  return <span className="inbox-editor-shortcut"><kbd>{shortcut}</kbd>  {t("inbox.submit")}</span>;
}
