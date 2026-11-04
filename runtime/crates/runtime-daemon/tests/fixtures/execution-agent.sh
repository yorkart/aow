#!/bin/sh
set -eu
session=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '--session-id' ]; then
    shift
    session="$1"
  fi
  shift
done
printf 'started\n' >> "$EXECUTION_COUNT_FILE"
if [ -n "${EXECUTION_WAIT_FILE:-}" ]; then
  while [ ! -e "$EXECUTION_WAIT_FILE" ]; do sleep 0.05; done
fi
cat > "$EXECUTION_PROMPT_FILE"
mkdir -p "$CLAUDE_CONFIG_DIR/projects/execution"
timestamp=$(date -u '+%Y-%m-%dT%H:%M:%S.000Z')
transcript="$CLAUDE_CONFIG_DIR/projects/execution/$session.jsonl"
printf '{"type":"user","uuid":"user","sessionId":"%s","cwd":"%s","timestamp":"%s","message":{"role":"user","content":"run"}}\n' "$session" "$PWD" "$timestamp" > "$transcript"
printf '{"type":"assistant","uuid":"assistant","parentUuid":"user","sessionId":"%s","cwd":"%s","timestamp":"%s","message":{"role":"assistant","content":[{"type":"text","text":"completed on execution node"}]}}\n' "$session" "$PWD" "$timestamp" >> "$transcript"
printf '{"type":"system","subtype":"turn_duration","sessionId":"%s","timestamp":"%s"}\n' "$session" "$timestamp" >> "$transcript"
printf 'node stdout\n'
printf 'node stderr\n' >&2
