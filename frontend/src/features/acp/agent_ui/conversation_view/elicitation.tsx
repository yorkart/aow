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
    if (Array.isArray(required) && required.includes(name) && (value == null || value === '' || Array.isArray(value) && !value.length)) return `${title}：请填写此项。`;
    if (value == null || value === '') continue;
    if (Array.isArray(value)) {
      if (value.length < (numeric(property.minItems) ?? 0) || value.length > (numeric(property.maxItems) ?? Infinity)) return `${title}：所选数量不符合要求。`;
    } else if (typeof value === 'string') {
      if (value.length < (numeric(property.minLength) ?? 0) || value.length > (numeric(property.maxLength) ?? Infinity)) return `${title}：长度不符合要求。`;
      if (property.pattern) {
        try { if (!new RegExp(text(property.pattern)).test(value)) return `${title}：格式不符合要求。`; }
        catch { return `${title}：Agent 提供的格式规则无效。`; }
      }
      if (property.format === 'date-time' && (!/T.+(?:Z|[+-]\d{2}:\d{2})$/.test(value) || !Number.isFinite(Date.parse(value)))) return `${title}：请输入包含时区的 ISO 日期时间。`;
    }
  }
  return '';
}

export function PermissionRequest({ permission, busy, onAnswer, embedded = false, requester = 'Agent' }: { permission: Permission; busy: boolean; onAnswer: (response: Data) => void; embedded?: boolean; requester?: string }) {
  const [values, setValues] = useState<Data>(() => initialValues(permission.request));
  const [error, setError] = useState('');
  const formId = useId();
  const request = permission.request;
  if (permission.kind === 'permission') return <section className={embedded ? 'acp-tool-permission' : 'acp-request-card'} aria-label="Agent 权限请求">
    {!embedded && <><header className="acp-request-header"><ShieldCheck size={14} /><span>权限请求</span><small>等待授权</small></header>
      <div className="acp-request-body"><p className="acp-request-message">{text(object(request.toolCall).title) || 'Agent 请求授权'}</p>
        {object(request.toolCall).rawInput != null && <details className="acp-request-details"><summary>请求详情</summary><pre>{JSON.stringify(object(request.toolCall).rawInput, null, 2)}</pre></details>}
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
  return <form className="acp-request-card acp-elicitation" aria-label="Agent 信息请求" onSubmit={event => {
    event.preventDefault();
    const issue = validationError(properties, values, schema.required); setError(issue);
    if (!issue && !unsupported) onAnswer({ action: 'accept', ...(request.mode === 'url' ? {} : { content: values }) });
  }}>
    <header className="acp-request-header"><MessageCircleQuestion size={14} /><span>来自 {requester} 的信息请求</span><small>等待回答</small></header>
    <div className="acp-request-body">
    <p className="acp-request-message">{text(request.message)}</p>
    {request.mode === 'url' && /^https?:\/\//.test(text(request.url)) && <a className="acp-request-url" href={text(request.url)} target="_blank" rel="noreferrer"><ExternalLink size={14} /><span>打开授权页面<small>{text(request.url)}</small></span></a>}
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
              : <span>暂不支持此字段类型</span>}
      </fieldset>;
    })}
    {unsupported && <p role="alert">此请求包含暂不支持的输入类型。</p>}{error && <p role="alert">{error}</p>}
    </div>
    <div className="acp-actions acp-request-actions"><button className="acp-request-submit" type="submit" disabled={busy || unsupported}><Check size={13} />确认</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'decline' })}><X size={13} />拒绝</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'cancel' })}>取消</button></div>
  </form>;
}
