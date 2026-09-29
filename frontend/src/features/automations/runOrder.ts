// Match the server's pagination/retention order across both stored ID formats.
// Decode with BigInt because the 63-bit ID cannot fit in a JS number.
function timestamp(id: string): number {
  if (/^[1-9a-z][0-9a-z]{11,12}$/.test(id)) {
    let value = 0n;
    for (const digit of id) value = value * 36n + BigInt(parseInt(digit, 36));
    if (value <= 0x7fffffffffffffffn) return Number(value >> 22n) + 1288834974657;
  }
  const legacy = /^(\d{4})(\d{2})(\d{2})T(\d{2})(\d{2})(\d{2})(\d{3})Z_/.exec(id);
  if (legacy) {
    const [, year, month, day, hour, minute, second, millis] = legacy;
    const value = Date.parse(`${year}-${month}-${day}T${hour}:${minute}:${second}.${millis}Z`);
    if (Number.isFinite(value)) return value;
  }
  return 0;
}

export function newestRunFirst(left: { id: string }, right: { id: string }): number {
  return timestamp(right.id) - timestamp(left.id) || (left.id < right.id ? 1 : left.id > right.id ? -1 : 0);
}
