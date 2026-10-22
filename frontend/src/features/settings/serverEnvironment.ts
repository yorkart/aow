import { t } from '../../lib/i18n';
interface EnvironmentField {
  key: string;
  description: string;
  example: string;
}

export const serverEnvironmentGroups: { title: string; fields: EnvironmentField[] }[] = [
  { title: t("settings.service_configuration"), fields: [
    { key: 'AOW_SERVER_HOST', description: t("settings.listen_address_default_127_0_0_1_use_0"), example: '127.0.0.1' },
    { key: 'AOW_SERVER_PORT', description: t("settings.listen_port_default_8282"), example: '8282' },
    { key: 'AOW_SERVER_STATE_DIR', description: t("settings.absolute_service_data_directory_takes_precedence_over_aow_state"), example: '/srv/aow/state' },
    { key: 'AOW_STATE_DIR', description: t("settings.aow_data_directory_defaults_to_aow_under_xdg_state"), example: '/srv/aow/state' },
    { key: 'AOW_TERMINALD_SOCKET', description: t("settings.absolute_terminald_unix_socket_path_must_match_terminald_s"), example: '/run/user/1000/aow-terminald/terminald.sock' },
    { key: 'AOW_BASE_PATH', description: t("settings.url_base_path_for_reverse_proxy_deployments_defaults_to"), example: '/tools/aow' },
  ] },
  { title: t("settings.login_sessions"), fields: [
    { key: 'AOW_AUTH_SECURE_COOKIE', description: t("settings.send_login_cookies_only_over_https_use_true_or"), example: 'true' },
    { key: 'AOW_AUTH_SESSION_MAX_AGE_SECONDS', description: t("settings.maximum_login_lifetime_in_positive_integer_seconds_unaffected_by"), example: '43200' },
    { key: 'AOW_AUTH_SESSION_IDLE_TIMEOUT_SECONDS', description: t("settings.idle_login_timeout_in_positive_integer_seconds_reset_by"), example: '1800' },
  ] },
  { title: t("settings.base_runtime_environment"), fields: [
    { key: 'XDG_STATE_HOME', description: t("settings.absolute_user_state_directory_default_local_state_aow_uses"), example: '/home/aow/.local/state' },
    { key: 'XDG_RUNTIME_DIR', description: t("settings.absolute_user_runtime_directory_the_default_terminald_socket_is"), example: '/run/user/1000' },
    { key: 'PATH', description: t("settings.colon_separated_absolute_directories_used_by_the_service_to"), example: '/home/aow/.local/bin:/usr/local/bin:/usr/bin:/bin' },
    { key: 'SHELL', description: t("settings.absolute_path_to_the_shell_executable_in_the_service"), example: '/bin/zsh' },
    { key: 'LANG', description: t("settings.default_language_and_character_encoding_use_a_utf_8"), example: 'en_US.UTF-8' },
    { key: 'LC_ALL', description: t("settings.overrides_all_language_and_character_encoding_settings_taking_precedence"), example: 'en_US.UTF-8' },
    { key: 'LC_CTYPE', description: t("settings.character_classification_and_encoding_overrides_lang_lc_all_takes"), example: 'en_US.UTF-8' },
  ] },
  { title: t("settings.proxy_configuration"), fields: [
    ...[
      { key: 'HTTP_PROXY', description: t("settings.proxy_url_for_http_requests"), example: 'http://127.0.0.1:7890' },
      { key: 'HTTPS_PROXY', description: t("settings.proxy_url_for_https_requests_http_proxy_urls_can"), example: 'http://127.0.0.1:7890' },
      { key: 'ALL_PROXY', description: t("settings.general_proxy_url_protocol_support_and_priority_depend_on"), example: 'socks5://127.0.0.1:7890' },
      { key: 'NO_PROXY', description: t("settings.comma_separated_hosts_domains_or_ips_that_bypass_the"), example: 'localhost,127.0.0.1,::1' },
    ].flatMap(field => [field, { ...field, key: field.key.toLowerCase(), description: t("settings.lowercase_form_for_clients_that_read_lowercase_variables_saved", { value0: field.description }) }]),
  ] },
];

export const serverEnvironmentFields = serverEnvironmentGroups.flatMap(group => group.fields);
export type ServerEnvironmentValues = Record<string, string>;
const keys = new Set(serverEnvironmentFields.map(field => field.key));
const proxyKey = /^(http|https|all|no)_proxy$/i;
const assignment = /^([ \t]*([A-Za-z_][A-Za-z0-9_]*)[ \t]*=[ \t]*)(.*?)([ \t]*)$/s;

function continues(value: string) {
  const quote = value[0];
  if (quote === "'" || quote === '"') {
    for (let index = 1; index < value.length; index++) {
      if (quote === '"' && value[index] === '\\') index++;
      else if (value[index] === quote) return false;
    }
    return true;
  }
  return (value.match(/\\+$/)?.[0].length ?? 0) % 2 === 1;
}

function lines(content: string, platform: string) {
  const physical = content.match(/[^\n]*\n|[^\n]+$/g) ?? [];
  const entries: { raw: string; ending: string; match: RegExpMatchArray | null }[] = [];
  for (let index = 0; index < physical.length; index++) {
    let raw = physical[index];
    for (;;) {
      const ending = raw.endsWith('\r\n') ? '\r\n' : raw.endsWith('\n') ? '\n' : '';
      const text = ending ? raw.slice(0, -ending.length) : raw;
      const match = text.match(assignment);
      // Keep systemd multiline entries together, including unmanaged variables,
      // so a line inside a quoted value is never edited as another assignment.
      if (platform === 'linux' && match && !proxyKey.test(match[2]) && continues(match[3]) && index + 1 < physical.length) {
        raw += physical[++index];
      } else {
        entries.push({ raw, ending, match });
        break;
      }
    }
  }
  return entries;
}

function valueOf(raw: string, systemd: boolean) {
  const quoted = raw.match(/^(['"])(.*)\1$/s);
  const value = quoted ? quoted[2] : raw;
  // launchd and the proxy launcher read literal values, without shell expansion.
  if (!systemd || quoted?.[1] === "'") return value;
  const joined = value.replace(/\\\r?\n/g, '');
  return quoted ? joined.replace(/\\([\\"`$])/g, '$1') : joined.replace(/\\(.)/g, '$1');
}

export function readServerEnvironment(content: string, platform: string): ServerEnvironmentValues {
  const values: ServerEnvironmentValues = {};
  for (const { match } of lines(content, platform)) {
    if (match && keys.has(match[2])) values[match[2]] = valueOf(match[3], platform === 'linux' && !proxyKey.test(match[2]));
  }
  return values;
}

export function validateServerEnvironment(values: ServerEnvironmentValues): ServerEnvironmentValues {
  const errors: ServerEnvironmentValues = {};
  for (const { key } of serverEnvironmentFields) {
    const value = values[key] ?? '';
    if (!value) continue;
    if (/[\r\n\0]/.test(value)) errors[key] = t("settings.enter_a_single_line_value_without_newlines_or_null");
    else if (key === 'AOW_SERVER_PORT' && (!/^\d+$/.test(value) || Number(value) > 65535)) errors[key] = t("settings.port_must_be_an_integer_from_0_to_65535");
    else if (key === 'AOW_AUTH_SECURE_COOKIE' && !['true', 'false'].includes(value)) errors[key] = t("settings.enter_true_or_false");
    else if (key.startsWith('AOW_AUTH_SESSION_') && (!/^\d+$/.test(value) || BigInt(value) === 0n || BigInt(value) > 18446744073709551615n)) errors[key] = t("settings.enter_a_positive_integer_number_of_seconds_up_to");
  }
  return errors;
}

function encodeValue(value: string, previous: string, systemd: boolean) {
  const quote = previous.match(/^(['"])(.*)\1$/s)?.[1];
  if (quote === "'" && !value.includes("'")) return `'${value}'`;
  if (!quote && !/[\s'"\\]/.test(value)) return value;
  if (quote !== '"' && !value.includes("'")) return `'${value}'`;
  return `"${systemd ? value.replace(/[\\"`$]/g, '\\$&') : value}"`;
}

export function writeServerEnvironment(content: string, values: ServerEnvironmentValues, platform: string): string {
  const original = readServerEnvironment(content, platform);
  const changed = new Set(serverEnvironmentFields.filter(({ key }) => (values[key] ?? '') !== (original[key] ?? '')).map(({ key }) => key));
  const errors = validateServerEnvironment(Object.fromEntries([...changed].map(key => [key, values[key]])));
  if (Object.keys(errors).length) throw new Error(Object.entries(errors).map(([key, error]) => `${key}：${error}`).join('\n'));
  const entries = lines(content, platform);
  const last = new Map<string, number>();
  entries.forEach(({ match }, index) => { if (match) last.set(match[2], index); });
  // Only rewrite edited keys. Removing every duplicate prevents an older value resurfacing.
  let result = entries.map(({ raw, ending, match }, index) => {
    if (!match || !changed.has(match[2])) return raw;
    const key = match[2];
    const value = values[key] ?? '';
    if (!value || last.get(key) !== index) return '';
    return `${match[1]}${encodeValue(value, match[3], platform === 'linux' && !proxyKey.test(key))}${match[4]}${ending}`;
  }).join('');
  const ending = content.includes('\r\n') ? '\r\n' : '\n';
  for (const key of changed) {
    const value = values[key] ?? '';
    if (last.has(key) || !value) continue;
    if (result && !result.endsWith('\n')) result += ending;
    result += `${key}=${encodeValue(value, '', platform === 'linux' && !proxyKey.test(key))}${ending}`;
  }
  return result;
}
