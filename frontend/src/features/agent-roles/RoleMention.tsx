import { mentionParts, roleInitial } from './mentions';
import './role-mentions.css';

export function RoleMentionText({ text }: { text: string }) {
  return <>{mentionParts(text).map((part, index) => part.type === 'text' ? part.text : <span key={index} className="role-mention" title={part.id} data-role-id={part.id}><span className="role-avatar" aria-hidden="true">{roleInitial(part.name)}</span><span>@{part.name}</span></span>)}</>;
}
