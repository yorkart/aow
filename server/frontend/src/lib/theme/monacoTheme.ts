import type { editor } from 'monaco-editor';
import type { Theme } from './types';
import { resolveDiffPalette } from './diffPalette';

/** Adapter kept separate so choosing a theme never loads Monaco eagerly. */
export function monacoTheme(theme: Theme): editor.IStandaloneThemeData {
  const c = theme.colors;
  const diff = resolveDiffPalette(theme.appearance);
  return {
    base: theme.appearance === 'dark' ? 'vs-dark' : 'vs',
    inherit: true,
    rules: [
      { token: '', foreground: theme.editor.foreground.slice(1, 7) },
      ...Object.entries(theme.syntax).map(([token, color]) => ({ token, foreground: color.slice(1, 7) })),
      { token: 'type.identifier', foreground: theme.syntax.type.slice(1, 7) },
      { token: 'identifier', foreground: theme.syntax.variable.slice(1, 7) },
      { token: 'attribute.name', foreground: theme.syntax.variable.slice(1, 7) },
      { token: 'delimiter', foreground: theme.syntax.operator.slice(1, 7) },
    ],
    colors: {
      'editor.background': theme.editor.background,
      'editor.foreground': theme.editor.foreground,
      'editorLineNumber.foreground': theme.editor.lineNumber,
      'editorLineNumber.activeForeground': theme.editor.foreground,
      'editor.lineHighlightBackground': theme.editor.line,
      'editor.selectionBackground': theme.editor.selection,
      'editor.inactiveSelectionBackground': theme.editor.selection,
      'editorCursor.foreground': theme.editor.cursor,
      'editorGutter.background': theme.editor.background,
      'editorWidget.background': c.elevated,
      'editorWidget.foreground': c.text,
      'editorWidget.border': c.controlBorder,
      'editorSuggestWidget.background': c.elevated,
      'editorSuggestWidget.foreground': c.text,
      'editorSuggestWidget.selectedBackground': c.selection,
      'editorHoverWidget.background': c.elevated,
      'editorHoverWidget.foreground': c.text,
      'editorHoverWidget.border': c.controlBorder,
      'editorError.foreground': c.error,
      'editorWarning.foreground': c.warning,
      'editorInfo.foreground': c.info,
      'input.background': c.input,
      'input.foreground': c.text,
      'input.border': c.controlBorder,
      'focusBorder': c.accent,
      'diffEditor.insertedTextBackground': diff.insertedText,
      'diffEditor.removedTextBackground': diff.removedText,
      'diffEditor.insertedLineBackground': diff.insertedLine,
      'diffEditor.removedLineBackground': diff.removedLine,
      'diffEditorGutter.insertedLineBackground': diff.insertedGutter,
      'diffEditorGutter.removedLineBackground': diff.removedGutter,
      'diffEditorOverview.insertedForeground': diff.insertedOverview,
      'diffEditorOverview.removedForeground': diff.removedOverview,
    },
  };
}
