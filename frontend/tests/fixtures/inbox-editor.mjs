export async function fillInboxEditor(input, text) {
  await input.focus();
  await input.press('ControlOrMeta+A');
  if (text) await input.page().keyboard.insertText(text);
  else await input.press('Backspace');
}

export async function inboxEditorState(input) {
  return input.evaluate(async node => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    const editor = monaco.editor.getEditors().find(instance => instance.getDomNode()?.contains(node));
    const options = monaco.editor.EditorOption;
    return {
      value: editor.getValue(), language: editor.getModel().getLanguageId(),
      layout: editor.getLayoutInfo(), contentHeight: editor.getContentHeight(),
      minimap: editor.getOption(options.minimap).enabled, glyphMargin: editor.getOption(options.glyphMargin),
      folding: editor.getOption(options.folding), wordWrap: editor.getOption(options.wordWrap),
    };
  });
}

export async function inboxEditorValue(input) { return (await inboxEditorState(input)).value; }
