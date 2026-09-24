# AOW operation log

Shared append-only operation history, independent of HTTP, task execution and UI.
`Writer` uses `tracing-appender` to write UTF-8 JSONL files named
`operations.YYYY-MM-DD-HH.log` (UTC, hourly rotation). The server retains the
latest 720 files. Files are appended to, never copy-truncated or rewritten.
No database, external search executable, or persistent index is required.

`Reader::read(ReadOptions)` returns newest-first records with:

- `items`: at most `limit` records (default 50, maximum 200).
- `next_cursor`: `{ file, offset }`, an exclusive byte boundary for the next
  backwards scan. `null` means the retained history has been exhausted.
- `budget_exhausted`: the scan stopped at its byte budget, even if the result
  page is empty. Continue using the cursor to search older records.
- `scanned_bytes`: actual bytes read, including nonmatching records and block
  read-ahead; bounded by `scan_bytes` (default 4 MiB, maximum 16 MiB).

The next cursor advances by scanned records, not matching records. Filters
must stay unchanged while following a cursor; start a fresh query after changing
filters. Appends after the first page do not change older pages. Missing or
truncated cursor files return `CursorExpired`. Rotation crosses into older
files in filename order. Directory enumeration is bounded operationally by the
writer's retention policy; the byte budget applies to file contents.

Records are limited to 64 KiB. Reads use 16 KiB backwards blocks with bounded
line buffering. Torn final lines, oversized lines and invalid JSON are skipped.
A partly read valid line is retried on the next page; the minimum scan budget
exceeds the largest valid line to guarantee forward progress. Oversized or torn
lines can advance a cursor through their contents without buffering them whole.
The writer separates a torn final record when reopening the current file.

Writing and reading are synchronous: use worker threads from an async service.
Append returns after flushing the userspace writer; it does not promise an
`fsync` or power-loss durability. Use one writer per log directory. The server
serializes writes on a dedicated thread with a bounded queue, reports queue or
write failures to the UI, and drains accepted records when the service is dropped.

The server's operation service owns active state in memory and temporarily
retains completed results for clients to consume. It never resumes work from
these files. `operation_id` groups related events and `boot_id` identifies the
server instance that observed them. A missing completion record does not imply
success or failure. Task prompts, environment variables and terminal output
are not operation-log fields.
