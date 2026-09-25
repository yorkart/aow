import { useId } from 'react';
import { normalizeArguments, pasteArguments, type ArgumentsDraft } from './arguments';

export function AgentArgumentsInput({ value, onChange, executable, disabled, onError }: {
  value: ArgumentsDraft;
  onChange: (value: ArgumentsDraft) => void;
  executable: string;
  disabled: boolean;
  onError: (message: string) => void;
}) {
  const helpId = useId();
  return <>
    <label className="project-aow-dialog-field">
      <span>Arguments</span>
      <textarea className="project-aow-dialog-monospace" rows={6} spellCheck={false} autoCapitalize="off" autoCorrect="off"
        aria-label="Arguments" aria-describedby={helpId} value={value.text} disabled={disabled} placeholder={'-m\ngpt-6-luna\n-c\napproval_policy="on-request"'}
        onChange={event => { onChange({ ...value, text: event.target.value }); onError(''); }}
        onBlur={() => {
          try { onChange(normalizeArguments(value, executable)); onError(''); }
          catch (reason) { onError(reason instanceof Error ? reason.message : String(reason)); }
        }}
        onPaste={event => {
          const input = event.currentTarget;
          const text = event.clipboardData.getData('text/plain');
          if (!text) return;
          try {
            const pasted = pasteArguments(value, text, input.selectionStart, input.selectionEnd, executable);
            event.preventDefault();
            onChange(pasted.draft);
            onError('');
            requestAnimationFrame(() => { if (input.isConnected) input.setSelectionRange(pasted.caret, pasted.caret); });
          } catch (reason) {
            // Keep incomplete command text editable instead of dropping a failed paste.
            onError(reason instanceof Error ? reason.message : String(reason));
          }
        }} />
    </label>
    <p className="project-aow-form-intro" id={helpId}>每行一个参数；可粘贴命令，自动识别引号、空格和行末 \ 续行。手动输入后离开输入框时自动整理。</p>
  </>;
}
