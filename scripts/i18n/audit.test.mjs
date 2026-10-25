import assert from 'node:assert/strict';
import { test } from 'node:test';
import { validateFrontendSource, validateRustSource } from './audit.mjs';

const resources = { greeting: 'Hello {{name}}', other: 'Other {{name}}', nested: '{{duration}} {{exit}}', seconds: '{{value}}' };
const frontend = source => validateFrontendSource(source, 'fixture.tsx', resources, new Set(['JSON']));

test('rejects English in conditional, fallback and template markup while preserving technical terms and predicates', () => {
  for (const source of [
    '<input placeholder={`Message ${agent}`} />',
    '<span>{branch || "Detached HEAD"}</span>',
    '<input title={ready ? "Ready" : "Waiting"} />',
    '<span>{ready && "Ready"}</span>',
  ]) assert.throws(() => frontend(source), /hardcoded English markup/u);
  assert.doesNotThrow(() => frontend('<span>{state === "ready" ? t("greeting", { name }) : "JSON"}</span>'));
});

test('validates both conditional translation keys and required interpolation arguments', () => {
  assert.throws(() => frontend('t(ready ? "greeting" : "missing", { name })'), /unknown translation key missing/u);
  assert.throws(() => frontend('t("greeting", {})'), /missing parameter name/u);
  assert.throws(() => frontend('t("greeting")'), /missing parameter name/u);
  assert.doesNotThrow(() => frontend('t(ready ? "greeting" : "other", { name })'));
});

test('backend parameters survive nested message calls, brackets and escaped quotes', () => {
  assert.throws(() => validateRustSource('aow_i18n::text("greeting")', 'crates/server/src/fixture.rs', resources), /text requires interpolation parameters/u);
  const source = String.raw`aow_i18n::message("nested", &[("duration", aow_i18n::message("seconds", &[("value", format!("[\"]"))])), ("exit", code.to_string())])`;
  assert.doesNotThrow(() => validateRustSource(source, 'crates/server/src/fixture.rs', resources));
  assert.throws(() => validateRustSource('aow_i18n::message("greeting", &[("duration", aow_i18n::message("greeting", &[("name", name.to_string())]))])', 'crates/server/src/fixture.rs', resources), /missing parameter name/u);
  assert.throws(() => validateRustSource(source.replace('("exit", code.to_string())', ''), 'crates/server/src/fixture.rs', resources), /missing parameter exit/u);
});

test('guards migrated Inbox errors and production Chinese while allowing test user content', () => {
  assert.throws(() => validateRustSource('ensure!(valid, "Inbox comments cannot be a symlink");', 'crates/server/src/inbox/persistence.rs', {}), /hardcoded persistence error/u);
  assert.throws(() => validateRustSource('let message = "请求失败";', 'crates/server/src/fixture.rs', {}), /hardcoded Chinese production string/u);
  assert.doesNotThrow(() => validateRustSource('#[cfg(test)] mod tests { let user = "用户内容"; }', 'crates/server/src/fixture.rs', {}));
});

test('follows local display constants through aliases without treating shadowed input as product text', () => {
  assert.throws(() => frontend('const prompt = `Message ${agent}`; const label = prompt; <input placeholder={label} />'), /hardcoded English markup/u);
  assert.throws(() => frontend('const title = ready ? "Ready" : "Waiting"; <span>{title}</span>'), /hardcoded English markup/u);
  assert.doesNotThrow(() => frontend('const title = "server-field"; function Display(title) { return <span>{title}</span>; }'));
  assert.doesNotThrow(() => frontend('const label = t("greeting", { name }); <input placeholder={label} />'));
});
