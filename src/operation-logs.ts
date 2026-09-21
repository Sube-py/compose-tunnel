export type OperationLogEntry = {
  id: string;
  timestamp: string;
  level: "info" | "error";
  operation: string;
  target: string;
  outcome: string;
  detail: string | null;
};

export function mergeOperationLogs(
  existing: OperationLogEntry[],
  incoming: OperationLogEntry[],
  limit = 200,
): OperationLogEntry[] {
  if (limit <= 0) {
    return [];
  }
  const byId = new Map([...existing, ...incoming].map((entry) => [entry.id, entry]));
  return [...byId.values()]
    .sort((a, b) => a.timestamp.localeCompare(b.timestamp) || a.id.localeCompare(b.id))
    .slice(-limit);
}

export function parseOperationLogMessage(message: string): OperationLogEntry | null {
  let value: unknown;
  try {
    value = JSON.parse(message);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return null;
  }
  const entry = value as Record<string, unknown>;
  if (
    typeof entry.id !== "string" || !entry.id ||
    typeof entry.timestamp !== "string" || !/^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$/.test(entry.timestamp) ||
    (entry.level !== "info" && entry.level !== "error") ||
    typeof entry.operation !== "string" ||
    typeof entry.target !== "string" ||
    typeof entry.outcome !== "string" ||
    (entry.detail !== null && typeof entry.detail !== "string")
  ) {
    return null;
  }
  return entry as OperationLogEntry;
}
