import { useState } from 'react';
import { list, object, text, type Data, type Permission } from '../../types';

function choices(property: Data, multiple = false): { value: string; label: string }[] | undefined {
  if (Array.isArray(property.enum)) return property.enum.map(value => ({ value: String(value), label: String(value) }));
  const titled = property[multiple ? 'anyOf' : 'oneOf'];
  if (Array.isArray(titled)) return list(titled).map(option => ({ value: text(option.const), label: text(option.title) || text(option.const) }));
}
const numeric = (value: unknown) => typeof value === 'number' ? value : undefined;
function initialValues(request: Data): Data {
  return Object.fromEntries(Object.entries(object(object(request.requestedSchema).properties)).flatMap(([name, raw]) => {
    const property = object(raw);
    if (property.default != null) return [[name, property.default]];
    return property.type === 'boolean' ? [[name, false]] : [];
  }));
}
function validationError(properties: Data, values: Data): string {
  for (const [name, raw] of Object.entries(properties)) {
    const property = object(raw); const value = values[name]; const title = text(property.title) || name;
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

export function PermissionRequest({ permission, busy, onAnswer }: { permission: Permission; busy: boolean; onAnswer: (response: Data) => void }) {
  const [values, setValues] = useState<Data>(() => initialValues(permission.request));
  const [error, setError] = useState('');
  const request = permission.request;
  if (permission.kind === 'permission') return <section className="zed-permission" aria-label="Agent 权限请求">
    <strong>{text(object(request.toolCall).title) || 'Agent 请求授权'}</strong>
    {object(request.toolCall).rawInput != null && <pre>{JSON.stringify(object(request.toolCall).rawInput, null, 2)}</pre>}
    <div className="zed-actions">{list(request.options).map(option => <button key={text(option.optionId)} type="button" disabled={busy} onClick={() => onAnswer({ outcome: { outcome: 'selected', optionId: option.optionId } })}>{text(option.name)}</button>)}
      <button type="button" disabled={busy} onClick={() => onAnswer({ outcome: { outcome: 'cancelled' } })}>取消</button></div>
  </section>;
  const schema = object(request.requestedSchema);
  const properties = object(schema.properties);
  const unsupported = !['form', 'url'].includes(text(request.mode)) || Object.values(properties).some(value => {
    const property = object(value);
    return !['string', 'number', 'integer', 'boolean', 'array'].includes(text(property.type)) || property.type === 'array' && !choices(object(property.items), true);
  });
  return <form className="zed-permission" aria-label="Agent 信息请求" onSubmit={event => {
    event.preventDefault();
    const issue = validationError(properties, values); setError(issue);
    if (!issue && !unsupported) onAnswer({ action: 'accept', ...(request.mode === 'url' ? {} : { content: values }) });
  }}>
    <strong>{text(request.message)}</strong>
    {request.mode === 'url' && /^https?:\/\//.test(text(request.url)) && <a href={text(request.url)} target="_blank" rel="noreferrer">打开授权页面</a>}
    {Object.entries(properties).map(([name, raw]) => {
      const property = object(raw); const required = Array.isArray(schema.required) && schema.required.includes(name);
      const options = choices(property); const multiple = choices(object(property.items), true);
      const setValue = (value: unknown) => setValues(current => { const next = { ...current }; if (value === undefined) delete next[name]; else next[name] = value; return next; });
      return <label key={name}>{text(property.title) || name}{required ? ' *' : ''}
        {property.type === 'boolean' ? <input aria-label={text(property.title) || name} type="checkbox" disabled={busy} checked={values[name] === true} onChange={event => setValue(event.target.checked)} />
          : options ? <select aria-label={text(property.title) || name} disabled={busy} required={required} value={text(values[name])} onChange={event => setValue(event.target.value || undefined)}><option value="">请选择</option>{options.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}</select>
            : property.type === 'array' && multiple ? <select aria-label={text(property.title) || name} disabled={busy} multiple required={required} value={Array.isArray(values[name]) ? values[name] as string[] : []} onChange={event => setValue(Array.from(event.target.selectedOptions, option => option.value))}>{multiple.map(option => <option key={option.value} value={option.value}>{option.label}</option>)}</select>
              : ['string', 'number', 'integer'].includes(text(property.type)) ? <input aria-label={text(property.title) || name} disabled={busy} required={required} type={property.type === 'string' ? ({ email: 'email', uri: 'url', date: 'date' })[text(property.format)] || 'text' : 'number'} min={numeric(property.minimum)} max={numeric(property.maximum)} minLength={numeric(property.minLength)} maxLength={numeric(property.maxLength)} step={property.type === 'integer' ? 1 : 'any'} value={typeof values[name] === 'number' ? values[name] : text(values[name])} onChange={event => setValue(event.target.value === '' ? undefined : property.type === 'string' ? event.target.value : event.target.valueAsNumber)} />
                : <span>暂不支持此字段类型</span>}
        {text(property.description) && <small>{text(property.description)}</small>}
      </label>;
    })}
    {unsupported && <p role="alert">此请求包含暂不支持的输入类型。</p>}{error && <p role="alert">{error}</p>}
    <div className="zed-actions"><button type="submit" disabled={busy || unsupported}>确认</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'decline' })}>拒绝</button><button type="button" disabled={busy} onClick={() => onAnswer({ action: 'cancel' })}>取消</button></div>
  </form>;
}
