import assert from 'node:assert/strict';

// Exercise the browser's actual composition path, including the soft-wrapped
// line whose old textarea buffer used to be drawn over the preceding text.
export const wrappedMarkdown = '### inbox\n\n1. 选择agent执行时，应该是从右侧弹出一个面板，显示 agent的选择、工作区方式选择、参考工作区、基础分支（类似automation 任务的配置），并且增加追加prompt的\n2. 添加需求宽度变小，padding变小，紧凑点';

export async function composeOnWrappedLine(input, { whileComposing = async () => {} } = {}) {
  const page = input.page();
  await input.focus();
  await input.evaluate(async node => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    const editor = monaco.editor.getEditors().find(editor => editor.getDomNode()?.contains(node));
    editor.setPosition({ lineNumber: 3, column: editor.getModel().getLineMaxColumn(3) });
    editor.revealPosition(editor.getPosition());
  });
  const geometry = () => input.evaluate(async node => {
    const { monaco } = await import('/src/features/editor/monaco.ts');
    const editor = monaco.editor.getEditors().find(editor => editor.getDomNode()?.contains(node));
    const root = editor.getDomNode(), bounds = root.getBoundingClientRect();
    const caret = editor.getScrolledVisiblePosition(editor.getPosition());
    const overlay = root.querySelector('textarea.ime-input');
    return { caret, top: bounds.top, scroll: editor.getScrollTop(), contentHeight: editor.getContentHeight(),
      overlayRight: overlay ? overlay.getBoundingClientRect().right - bounds.left : null,
      value: editor.getValue(), focused: node === document.activeElement };
  });
  const before = await geometry();
  const ime = await page.context().newCDPSession(page);
  try {
    for (const text of ['d', 'de', '的']) {
      await ime.send('Input.imeSetComposition', { text, selectionStart: text.length, selectionEnd: text.length });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      const during = await geometry();
      assert.equal(during.value, wrappedMarkdown.replace('prompt的', `prompt的${text}`), 'composition changes only the insertion point');
      assert.equal(during.focused, true);
      assert.equal(during.top, before.top, 'the editor does not jump while composing');
      assert.equal(during.caret.top, before.caret.top, 'the caret stays on the same wrapped line');
      assert.equal(during.scroll, before.scroll, 'composition does not scroll the editor');
      assert.equal(during.contentHeight, before.contentHeight);
      assert.ok(during.overlayRight === null || during.overlayRight <= during.caret.left + 3,
        'the IME layer must not paint the preceding wrapped text across the caret line');
    }
    await whileComposing();
    await ime.send('Input.insertText', { text: '的' });
    assert.equal((await geometry()).value, wrappedMarkdown.replace('prompt的', 'prompt的的'));
  } finally { await ime.detach(); }
}
