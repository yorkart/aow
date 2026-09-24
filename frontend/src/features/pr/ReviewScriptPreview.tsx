import '../editor/monaco';
import Editor from '@monaco-editor/react';

export default function ReviewScriptPreview({ value }: { value: string }) {
  return <div className="review-provider-editor"><Editor height="360px" language="python" theme="vs-dark" value={value}
    options={{ readOnly: true, domReadOnly: true, readOnlyMessage: { value: '脚本仅支持预览，请上传文件替换。' }, automaticLayout: true, minimap: { enabled: false }, fontSize: 13, scrollBeyondLastLine: false, tabFocusMode: true, ariaLabel: 'Provider Python 脚本预览', wordWrap: 'off' }} /></div>;
}
