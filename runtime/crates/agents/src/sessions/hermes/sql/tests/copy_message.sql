INSERT INTO messages(session_id, role, content, tool_call_id, tool_calls, timestamp, finish_reason)
SELECT session_id, role, CASE WHEN ?2 THEN
    '[PRIOR CONTEXT — for reference only; not a new message]' || char(10) || content || char(10) ||
    '[END OF PRIOR CONTEXT — COMPACTION SUMMARY BELOW]' || char(10) || '[CONTEXT SUMMARY]: older messages'
    ELSE content END, tool_call_id, tool_calls, ?3, finish_reason
FROM messages WHERE id = ?1
