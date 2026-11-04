import { t } from '../../../../lib/i18n';
import { useId, useState } from 'react';
import { Check, CheckCheck, ExternalLink, MessageCircleQuestion, ShieldCheck, X } from 'lucide-react';
import { list, object, text, type Data, type Permission } from '../../types';

function choices(property: Data, multiple = false): { value: string; label: string; description?: string }[] | undefined {
  if (Array.isArray(property.enum)) return property.enum.map(value => ({ value: String(value), label: String(value) }));
  const titled = property[multiple ? 'anyOf' : 'oneOf'];
  if (Array.isArray(titled)) return list(titled).map(option => ({ value: text(option.const), label: text(option.title) || text(option.const), description: text(option.description) }));
}
const numeric = (value: unknown) => typeof value === 'number' ? value : undefined;
function initialValues(request: Data): Data {
  return Object.fromEntries(Object.entries(object(object(request.requestedSchema).properties)).flatMap(([name, raw]) => {
    const property = object(raw);
    if (property.default != null) return [[name, property.default]];
    return property.type === 'boolean' ? [[name, false]] : [];
  }));
}
function validationError(properties: Data, values: Data, required: unknown): string {
  for (const [name, raw] of Object.entries(properties)) {
    const property = object(raw); const value = values[name]; const title = text(property.title) || name;
    if (Array.isArray(required) && required.includes(name) && (value == null || value === '' || Array.isArray(value) && !value.length)) return t("acp.this_field_is_required", { value0: title });
    if (value == null || value === '') continue;
    if (Array.isArray(value)) {
      if (value.length < (numeric(property.minItems) ?? 0) || value.length > (numeric(property.maxItems) ?? Infinity)) return t("acp.invalid_number_of_selections", { value0: title });
    } else if (typeof value === 'string') {
      if (value.length < (numeric(property.minLength) ?? 0) || value.length > (numeric(property.maxLength) ?? Infinity)) return t("acp.invalid_length", { value0: title });
      if (property.pattern) {
        try { if (!new RegExp(text(property.pattern)).test(value)) return t("acp.invalid_format", { value0: title }); }
        catch { return t("acp.the_agent_supplied_an_invalid_format_rule", { value0: title }); }
      }
      if (property.format === 'date-time' && (!/T.+(?:Z|[+-]\d{2}:\d{2})$/.test(value) || !Number.isFinite(Date.parse(value)))) return t("acp.enter_an_iso_date_and_time_including_a_time", { value0: title });
    }
  }
  return '';
}

export function PermissionRequest({ permission, busy, onAnswer, embedded = false, requester = 'Agent' }: { permission: Permission; busy: boolean; onAnswer: (response: Data) => void; embedded?: boolean; requester?: string }) {
  const [values, setValues] = useState<Data>(() => initialValues(permission.request));
  const [error, setError] = useState('');
  const formId = useId();
  const request = permission.request;
  if (permission.kind === 'permission') return <section className={embedded ? 'acp-tool-permission' : 'acp-request-card'} aria-label={t("acp.agent_permission_request")}>
    {!embedded && <><header className="acp-request-header"><ShieldCheck size={14} /><span>{t("acp.permission_request")}</span><small>{t("acp.awaiting_permission")}</small></header>
      <div className="acp-request-body"><p className="acp-request-message">{text(object(request.toolCall).title) || t("acp.agent_requests_permission")}</p>
        {object(request.toolCall).rawInput != null && <details className="acp-request-details"><summary>{t("acp.request_details")}</summary><pre>{JSON.stringify(object(request.toolCall).rawInput, null, 2)}</pre></details>}
      </div></>}
    <div className="acp-permission-options">{list(request.options).map(option => {
      const kind = text(option.kind);
      const Icon = kind === 'allow_always' ? CheckCheck : kind === 'allow_once' ? Check : kind.startsWith('reject') ? X : ShieldCheck;
      return <button key={text(option.optionId)} data-kind={kind} type="button" disabled={busy} onClick={() => onAnswer({ outcome: { outcome: 'selected', optionId: option.optionId } })}><Icon size={13} /><span>{text(option.name)}</span></button>;
    })}</div>
  </section>;
  const schema = object(request.requestedSchema);
  const properties = object(schema.properties);
  const unsupported = !['form', 'url'].includes(text(request.mode)) || Object.values(properties).some(value => {
    const property = object(value);
    return !['string', 'number', 'integer', 'boolean', 'array'].includes(text(property.type)) || property.type === 'array' && !choices(object(property.items), true);
  });
  return <form className="acp-request-card acp-elicitation" aria-label={t("acp.agent_information_request")} onSubmit={event => {
    event.preventDefault();
    const issue = validationError(properties, values, schema.required); setError(issue);
    if (!issue && !unsupported) onAnswer({ action: 'accept', ...(request.mode === 'url' ? {} : { content: values }) });
  }}>
    <header className="acp-request-header"><MessageCircleQuestion size={14} /><span>{t("acp.information_request_from")} {requester} {t("acp.message")}</span><small>{t("acp.awaiting_answer")}</small></header>
    <div className="acp-request-body">
    <p className="acp-request-message">{text(request.message)}</p>
    {request.mode === 'url' && /^https?:\/\//.test(text(request.url)) && <a className="acp-request-url" href={text(request.url)} target="_blank" rel="noreferrer"><ExternalLink size={14} /><span>{t("acp.open_authorization_page")}<small>{text(request.url)}</small></span></a>}
    {text(schema.title) && <p className="acp-request-schema-title">{text(schema.title)}</p>}
    {text(schema.description) && <p className="acp-request-description">{text(schema.description)}</p>}
    {Object.entries(properties).map(([name, raw]) => {
      const property = object(raw); const required = Array.isArray(schema.required) && schema.required.includes(name);
      const label = text(property.title) || name;
      const options = choices(property); const multiple = choices(object(property.items), true);
      const setValue = (value: unknown) => setValues(current => { const next = { ...current }; if (value === undefined) delete next[name]; else next[name] = value; return next; });
      const description = text(property.description);
      if (property.type === 'boolean') return <label className="acp-request-boolean" key={name}><input aria-label={label} type="checkbox" disabled={busy} checked={values[name] === true} onChange={event => setValue(event.target.checked)} /><span>{label}{description && <small>{description}</small>}</span></label>;
      return <fieldset className="acp-request-field" key={name} disabled={busy}>
        <legend>{label}{required && <span aria-hidden="true"> *</span>}</legend>
        {description && <p className="acp-request-description">{description}</p>}
        {options ? <div className="acp-choice-list" role="radiogroup" aria-label={label}>{options.map(option => <label className="acp-choice-card" key={option.value}>
          <input type="radio" name={`${formId}-${name}`} value={option.value} required={required} checked={values[name] === option.value} onChange={() => setValue(option.value)} /><span>{option.label}{option.description && <small>{option.description}</small>}</span>
        </label>)}</div>
          : property.type === 'array' && multiple ? <div className="acp-choice-list">{multiple.map(option => <label className="acp-choice-card" key={option.value}>
            <input type="checkbox" value={option.value} checked={Array.isArray(values[name]) && (values[name] as string[]).includes(option.value)} onChange={event => { const selected = Array.isArray(values[name]) ? values[name] as string[] : []; setValue(event.target.checked ? [...selected, option.value] : selected.filter(value => value !== option.value)); }} /><span>{option.label}{option.description && <small>{option.description}</small>}</span>
          </label>)}</div>
            : ['string', 'number', 'integer'].includes(text(property.type)) ? <input aria-label={label} required={required} type={property.type === 'string' ? ({ email: 'email', uri: 'url', date: 'date' })[text(property.format)] || 'text' : 'number'} min={numeric(property.minimum)} max={numeric(property.maximum)} minLength={numeric(property.minLength)} maxLength={numeric(property.maxLength)} step={property.type === 'integer' ? 1 : 'any'} value={typeof values[name] === 'number' ? values[name] : text(values[name])} onChange={event => setValue(event.target.value === '' ? undefined : property.type === 'string' ? event.target.value : event.target.valueAsNumber)} />
              : <span>{t("acp.unsupported_field_type")}</span>}
      </fieldset>;
    })}
    {unsupported && <p role="alert">{t("acp.this_request_contains_unsupported_input_types")}</p>}{error && <p role="alert">{error}</p>}
    </div>
    <div className="acp-actions acp-request-actions"><button className="acp-request-submit" type="submit" disabled={busy || unsupported}><Check size={13} />{t("components.confirm")}</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'decline' })}><X size={13} />{t("acp.deny")}</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'cancel' })}>{t("aow.cancel")}</button></div>
  </form>;
}
