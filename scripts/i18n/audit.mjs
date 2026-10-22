import assert from 'node:assert/strict';
import ts from 'typescript';

export const placeholders = text => [...text.matchAll(/\{\{([^{}]+)\}\}/gu)].map(match => match[1]).sort();

export function validateFrontendSource(text, file, resources, technicalTerms) {
  const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
  const declarations = new Map();
  const scopeOf = node => {
    while (node.parent && !ts.isBlock(node) && !ts.isSourceFile(node)) node = node.parent;
    return node;
  };
  const collect = node => {
    if (ts.isVariableDeclaration(node) && ts.isIdentifier(node.name) && node.initializer) {
      const scope = scopeOf(node);
      if (!declarations.has(scope)) declarations.set(scope, new Map());
      declarations.get(scope).set(node.name.text, node.initializer);
    }
    ts.forEachChild(node, collect);
  };
  collect(source);
  const visibleValues = (expression, seen = new Set()) => {
    if (!expression || seen.has(expression)) return [];
    seen = new Set(seen).add(expression);
    if (ts.isStringLiteral(expression) || ts.isNoSubstitutionTemplateLiteral(expression)) return [expression.text];
    if (ts.isTemplateExpression(expression)) return [expression.head.text, ...expression.templateSpans.flatMap(span => [...visibleValues(span.expression, seen), span.literal.text])];
    if (ts.isConditionalExpression(expression)) return [...visibleValues(expression.whenTrue, seen), ...visibleValues(expression.whenFalse, seen)];
    if (ts.isParenthesizedExpression(expression) || ts.isAsExpression(expression)) return visibleValues(expression.expression, seen);
    if (ts.isBinaryExpression(expression)) {
      if (expression.operatorToken.kind === ts.SyntaxKind.AmpersandAmpersandToken) return visibleValues(expression.right, seen);
      if ([ts.SyntaxKind.BarBarToken, ts.SyntaxKind.QuestionQuestionToken, ts.SyntaxKind.PlusToken].includes(expression.operatorToken.kind)) return [...visibleValues(expression.left, seen), ...visibleValues(expression.right, seen)];
    }
    if (ts.isIdentifier(expression)) {
      for (let scope = scopeOf(expression); scope; scope = scope.parent ? scopeOf(scope.parent) : undefined) {
        const initializer = declarations.get(scope)?.get(expression.text);
        if (initializer) return visibleValues(initializer, seen);
        const owner = scope.parent;
        if (owner && ts.isFunctionLike(owner) && owner.parameters.some(parameter => ts.isIdentifier(parameter.name) && parameter.name.text === expression.text)) return [];
      }
    }
    return [];
  };
  const visit = node => {
    if (ts.isCallExpression(node) && ['t', 'translate'].includes(node.expression.getText(source))) {
      const keys = expression => ts.isStringLiteral(expression) ? [expression.text] : ts.isConditionalExpression(expression) ? [...keys(expression.whenTrue), ...keys(expression.whenFalse)] : [];
      for (const key of node.arguments[0] ? keys(node.arguments[0]) : []) {
        assert.ok(Object.hasOwn(resources, key), `${file}: unknown translation key ${key}`);
        const options = node.arguments[1];
        const expected = [...new Set(placeholders(resources[key]))];
        if (expected.length && (!options || ts.isObjectLiteralExpression(options))) {
          const supplied = options?.properties.map(property => property.name?.getText(source).replace(/^['"]|['"]$/gu, '')) ?? [];
          for (const name of expected) assert.ok(supplied.includes(name) || options?.properties.some(ts.isSpreadAssignment), `${file}: ${key}: missing parameter ${name}`);
        }
      }
    }
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isJsxText(node) || ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node)) {
      assert.ok(!/\p{Script=Han}/u.test(node.text), `${file}: hardcoded Chinese text at line ${source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1}`);
    }
    let visibleAttribute = node.parent;
    let expressionValue = true;
    let child = node;
    while (visibleAttribute && !ts.isJsxAttribute(visibleAttribute) && !ts.isJsxElement(visibleAttribute) && !ts.isSourceFile(visibleAttribute)) {
      if (ts.isCallExpression(visibleAttribute) || ts.isBinaryExpression(visibleAttribute)
        && (![ts.SyntaxKind.BarBarToken, ts.SyntaxKind.AmpersandAmpersandToken, ts.SyntaxKind.QuestionQuestionToken].includes(visibleAttribute.operatorToken.kind) || child !== visibleAttribute.right)
        || ts.isConditionalExpression(visibleAttribute) && child === visibleAttribute.condition) expressionValue = false;
      child = visibleAttribute;
      visibleAttribute = visibleAttribute.parent;
    }
    const literalMarkup = ts.isJsxText(node) || (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isTemplateHead(node) || ts.isTemplateMiddle(node) || ts.isTemplateTail(node))
      && expressionValue && (ts.isJsxAttribute(visibleAttribute) && ['title', 'aria-label', 'alt', 'placeholder'].includes(visibleAttribute.name.text)
        || ts.isJsxElement(visibleAttribute) && ts.isJsxExpression(child));
    if (ts.isJsxExpression(node) && node.expression && (ts.isJsxElement(node.parent)
      || ts.isJsxAttribute(node.parent) && ['title', 'aria-label', 'alt', 'placeholder'].includes(node.parent.name.text))) {
      for (const value of visibleValues(node.expression)) {
        if (/[A-Za-z]{2}/u.test(value)) assert.ok(technicalTerms.has(value.trim()), `${file}: hardcoded English markup: ${value.trim()}`);
      }
    }
    if (literalMarkup && /[A-Za-z]{2}/u.test(node.text)) {
      assert.ok(technicalTerms.has(node.text.trim()), `${file}: hardcoded English markup: ${node.text.trim()}`);
    }
    ts.forEachChild(node, visit);
  };
  visit(source);
}

export function validateRustSource(source, file, resources) {
  for (const match of source.matchAll(/aow_i18n::(?:text|message)\(\s*"([^"]+)"/gu)) {
    assert.ok(Object.hasOwn(resources, match[1]), `${file}: unknown backend key ${match[1]}`);
  }
  for (const match of source.matchAll(/aow_i18n::text\(\s*"([^"]+)"/gu)) {
    assert.equal(placeholders(resources[match[1]]).length, 0, `${file}: ${match[1]}: text requires interpolation parameters; use message`);
  }
  for (const match of source.matchAll(/aow_i18n::message\(\s*"([^"]+)"\s*,\s*&\[/gu)) {
    let depth = 1;
    let quoted = false;
    let parentheses = 0;
    const supplied = [];
    let escaped = false;
    let end = match.index + match[0].length;
    for (; end < source.length && depth; end++) {
      const character = source[end];
      if (quoted) {
        if (escaped) escaped = false;
        else if (character === '\\') escaped = true;
        else if (character === '"') quoted = false;
      } else if (character === '"') quoted = true;
      else if (character === '(') {
        if (depth === 1 && parentheses === 0) {
          const parameter = /^\(\s*"([^"]+)"\s*,/u.exec(source.slice(end));
          if (parameter) supplied.push(parameter[1]);
        }
        parentheses++;
      } else if (character === ')') parentheses--;
      else if (character === '[') depth++;
      else if (character === ']') depth--;
    }
    for (const name of new Set(placeholders(resources[match[1]]))) {
      assert.ok(supplied.includes(name), `${file}: ${match[1]}: missing parameter ${name}`);
    }
  }
  // Unit fixtures may deliberately contain Chinese user content. Production Rust
  // must keep it in catalogues, just like the frontend.
  const production = source.split(/#\[cfg\(test\)\]/u)[0];
  if (!file.startsWith('crates/i18n/') && !/(?:^|\/)(?:tests?(?:\/|\.rs)|[^/]*_tests\.rs)/u.test(file)) {
    assert.ok(!/"[^"\n]*\p{Script=Han}[^"\n]*"/u.test(production), `${file}: hardcoded Chinese production string`);
  }
  // This module is fully migrated; guard its own error and context sinks.
  if (file === 'crates/server/src/inbox/persistence.rs') {
    assert.ok(!/(?:ensure!|bail!|anyhow!|\.context|\.with_context)\([\s\S]{0,200}?"[A-Z][A-Za-z ]+ [A-Za-z ]+(?:: [^"\n]*)?"/u.test(production), `${file}: hardcoded persistence error`);
  }
}
