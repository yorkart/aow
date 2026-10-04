import { useEffect, useState } from 'react';
import { MarkdownContent } from '../../components/MarkdownContent';
import { inboxApi } from './api';
import { inboxError, type InboxComment } from './types';

export function InboxComments({ itemId, count }: { itemId: string; count: number }) {
  const [comments, setComments] = useState<InboxComment[]>();
  const [error, setError] = useState('');
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    let active = true;
    setError('');
    void inboxApi.comments(itemId).then(next => {
      if (active) setComments(next);
    }).catch(reason => { if (active) setError(inboxError(reason)); });
    return () => { active = false; };
  }, [itemId, count, attempt]);
  return <section id={`inbox-comments-${itemId}`} className="inbox-comments" aria-label="需求评论">
    {!comments && !error && <p className="inbox-comments-loading" role="status">正在加载评论…</p>}
    {error && <p className="inbox-error" role="alert">{error}<button type="button" onClick={() => setAttempt(value => value + 1)}>重试加载评论</button></p>}
    {comments && <ol>{comments.map(comment => <li key={comment.id} data-comment-id={comment.id}>
      <header><span>{comment.author.name}</span><time dateTime={comment.created_at}>{new Date(comment.created_at).toLocaleString()}</time></header>
      <MarkdownContent text={comment.content} readOnly className="inbox-markdown" />
    </li>)}</ol>}
  </section>;
}
