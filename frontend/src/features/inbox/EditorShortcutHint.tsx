export function EditorShortcutHint() {
  const shortcut = /Mac|iPhone|iPad|iPod/.test(navigator.userAgent) ? '⌘ + Enter' : 'Ctrl + Enter';
  return <span className="inbox-editor-shortcut"><kbd>{shortcut}</kbd> 提交</span>;
}
