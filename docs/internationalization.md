# Internationalization

## Scope and baseline

This implementation starts at `weekly/2026-10-25`, commit `1e236b7`.
English (`en-US`) is the product default. Simplified Chinese (`zh-CN`) is an
explicitly supported alternative. Browser language does not override the product
default. Unsupported preferences fall back to English.

The initial audit is recorded in `internationalization-inventory.json`. Its file
positions describe the audit snapshot; subsequent edits change line numbers.
The audit includes 2,347 Chinese frontend text nodes (1,813 distinct messages),
followed by nested template expressions, English UI labels, and backend messages.
The frontend audit uses the TypeScript syntax tree, including accessibility
labels, placeholders, validation messages, and template parameters. The backend
inventory classifies embedded test fixtures separately from production messages
and HTML templates. Current language catalogues are the maintained source of
truth, including messages added while implementing the audit findings.

## Resource ownership

| Content | Owner |
| --- | --- |
| Application UI, validation, empty states, accessibility labels | `frontend/src/lib/i18n/locales/` |
| Backend validation, typed errors, operation messages, IM templates | `crates/i18n/locales/` |
| Editor menus and built-in editor accessibility messages | Matching Monaco language pack in `monacoLocale.ts` |
| User prompts, notes, session content, paths, titles | Original content; never translated |
| External process output, provider/agent supplied text and underlying OS errors | Original diagnostic content |
| Protocol codes, CLI flags, executable names, product names and command examples | Stable technical identifiers |

Resources use stable, semantic keys grouped by feature. Do not rename a key when
only its wording changes. Do not derive keys from the displayed translation.
Use complete sentences and named placeholders. For new counts, use i18next
`count` and `_one` / `_other` forms. Do not concatenate translated sentence
fragments. Existing technical names can legitimately be identical across locales.

## Frontend

Use `t('feature.message', { name: value })` from `src/lib/i18n`. React renders the
returned string as text; do not use HTML injection for translations. i18next is
configured with English fallback and synchronous initialization so module-level
labels and formatters use the saved language from their first evaluation.

Settings → Appearance includes the language selector. The preference is saved
under `aow.language` in this browser. Applying a different language explicitly
warns about unsaved work, then reloads the page. Reloading also refreshes static
labels, date/number formatters, editor localization, and existing server streams.
A storage failure leaves the current page intact and displays an error.

Use `getLanguage()` for `Intl.DateTimeFormat`, `Intl.RelativeTimeFormat`, and
`Intl.NumberFormat`. Language, time zone, and currency are independent choices;
the implementation does not infer a time zone or currency from language.

Application HTTP clients use `localizedFetch`, which adds `Accept-Language`
without patching global fetch. Native Headers and tuple-style HeadersInit are
supported. Unrelated request options and payloads are preserved.
Browser WebSocket URLs include `lang` with the application preference, since
native WebSockets cannot set custom request headers. The server accepts this
override only on WebSocket handshakes; ordinary HTTP uses `Accept-Language`.
All upgrade handlers use `crate::locale::upgrade` to carry the negotiated locale
into Axum's separately spawned callback, including terminal attach errors.

## Backend

Use `aow_i18n::text(key)` for a borrowed static message and
`aow_i18n::message(key, &[("name", value.to_string())])` for named parameters.
Parameters are inserted once and are never reinterpreted as translation syntax.
Keep existing machine-readable error codes and structured resource fields stable.
Translate display text at its source; do not infer error codes from translated
sentences or rewrite user data in arbitrary JSON responses.

HTTP middleware negotiates supported `Accept-Language` values and quality weights,
defaults to English, and adds `Content-Language` and `Vary: Accept-Language`.
Language is scoped per asynchronous task, not stored in a mutable process-global
locale. `aow_i18n::spawn` and `spawn_blocking` carry it across task boundaries;
blocking worker state is restored even when unwinding.

Settings → Notifications includes an independent notification language. It is
persisted in `notification-settings.json`, defaults to English for old documents,
and is preserved by older clients that omit the field on updates. Background
agent-completion, automation-failure, and authentication IM messages use this
preference even when no browser is open. User conclusions and diagnostic details
remain unchanged. Historical operation records and run snapshots retain the
language in which they were originally produced; they are not rewritten on a
later language change.

## Adding a language

1. Add a frontend catalogue with the same keys and placeholders as English, and
   register its language tag, native name, direction, and resources in the frontend
   runtime. Define any regional alias mapping explicitly.
2. Add the backend catalogue, extend `Language`, register its resource and supported
   HTTP negotiation tags. Configure a matching editor pack when available; otherwise
   keep an explicit English editor fallback.
3. Add language-selection, plural, format, API, and notification coverage. Run
   `npm run check:i18n`, typecheck, build, relevant browser tests, and Rust checks.

The current product has no account-level UI locale setting; UI preference is local
to the browser and notification preference belongs to the deployment's notification
configuration. A translation management platform can synchronize these checked-in
JSON files later without moving runtime text into a database.

## Validation and maintenance

`npm run check:i18n` verifies locale key parity, placeholder parity, static frontend
and backend translation references (including conditional frontend keys), required
parameters in static object/array arguments, and hardcoded Chinese production
literals. English JSX text, conditional/fallback display values, template literals,
simple local display constants, and display attributes must also use resources; technical names and code examples have an
explicit allowlist in `scripts/i18n/technical-terms.json`.
It is included in `npm run lint`, so the existing CI lint job runs it. It does not
claim to understand every arbitrary English string or translate external content.
Review new English display text for resource ownership during code review.
The fully migrated Inbox persistence module also rejects hardcoded English error
sinks. This is a scoped check, not a claim that arbitrary English Rust diagnostics
or display strings computed through arbitrary functions are statically understood.
Guard regression tests run as part of `check:i18n`.

`npm --prefix frontend run test:i18n` covers the English default, Chinese aliases,
blocked storage, interpolation, and request headers. Browser coverage in
`tests/i18n-browser.test.mjs` verifies explicit switching, persistence, document
language, request propagation, and storage failure. Rust tests cover negotiation,
concurrent language isolation, asynchronous/blocking propagation, interpolation,
and real WebSocket negotiation/message delivery after upgrade. Inbox error tests
verify both languages and preserve path parameters verbatim.

The initial broader regression run exposed pre-existing ACP layout and architecture
failures. These were independently reproduced in a worktree at the baseline commit;
the implementation does not claim that the entire legacy browser suite is green.
The execution report records the final checks and any remaining limitations.

A post-review run also reproduced the mobile per-pane agent-title assertion on the
unchanged baseline build. The migration does not change that title-selection
behavior. WebSocket test fixtures now match URLs with locale query parameters;
physical terminal key names such as Tab and Space remain keyboard identifiers.

Two mobile hosting tests (takeover cleanup and stopped-outcome refresh) also fail
unchanged on the original baseline. These are recorded separately from translated
selector updates and locale-transport regression tests.
