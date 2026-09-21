export type CommandStatus = "running" | "success" | "failure" | "timeout" | "stopped";

export type CommandLogEntry = {
  id: string;
  revision: number;
  started_at: string;
  updated_at: string;
  server: string;
  preview: string;
  status: CommandStatus;
  exit_code: number | null;
  duration_ms: number | null;
  detail_available: boolean;
};

export type CommandLogDetail = {
  id: string;
  program: string;
  args: string[];
  local_command: string;
  remote_command: string | null;
  stdout_base64: string;
  stderr_base64: string;
};

export function mergeCommandLogs(existing: CommandLogEntry[], incoming: CommandLogEntry[], limit = 200): CommandLogEntry[] {
  if (limit <= 0) return [];
  const byId = new Map<string, CommandLogEntry>();
  for (const entry of [...existing, ...incoming]) {
    const old = byId.get(entry.id);
    if (!old || entry.revision >= old.revision) byId.set(entry.id, entry);
  }
  return [...byId.values()]
    .sort((a, b) => b.started_at.localeCompare(a.started_at) || b.id.localeCompare(a.id))
    .slice(0, limit);
}

export function parseCommandLogMessage(message: string): CommandLogEntry | null {
  let value: unknown;
  try { value = JSON.parse(message); } catch { return null; }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const row = value as Record<string, unknown>;
  if (
    typeof row.id !== "string" || !row.id ||
    !Number.isSafeInteger(row.revision) || (row.revision as number) < 0 ||
    typeof row.started_at !== "string" || Number.isNaN(Date.parse(row.started_at)) ||
    typeof row.updated_at !== "string" || Number.isNaN(Date.parse(row.updated_at)) ||
    typeof row.server !== "string" || typeof row.preview !== "string" ||
    !["running", "success", "failure", "timeout", "stopped"].includes(String(row.status)) ||
    (row.exit_code !== null && !Number.isInteger(row.exit_code)) ||
    (row.duration_ms !== null && (!Number.isSafeInteger(row.duration_ms) || (row.duration_ms as number) < 0)) ||
    typeof row.detail_available !== "boolean"
  ) return null;
  return row as CommandLogEntry;
}

export function isCommandLogPersistenceError(message: string): boolean {
  try {
    const value: unknown = JSON.parse(message);
    return typeof value === "object" && value !== null && !Array.isArray(value) &&
      (value as Record<string, unknown>).kind === "persistence_error";
  } catch {
    return false;
  }
}

export function decodeOutput(base64: string): string {
  const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return Array.from(bytes, (byte) => `\\x${byte.toString(16).toUpperCase().padStart(2, "0")}`).join("");
  }
}
