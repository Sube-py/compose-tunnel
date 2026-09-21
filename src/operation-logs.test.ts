import { describe, expect, it } from "vitest";
import {
  mergeOperationLogs,
  parseOperationLogMessage,
  type OperationLogEntry,
} from "./operation-logs";

const entry = (id: string, timestamp: string): OperationLogEntry => ({
  id,
  timestamp,
  level: "info",
  operation: "Docker inspect",
  target: "staging/app/db",
  outcome: "success",
  detail: null,
});

describe("operation log history and live events", () => {
  it("deduplicates the same entry across history and live delivery", () => {
    const first = entry("1", "2026-09-21T01:00:00.000Z");
    const second = entry("2", "2026-09-21T01:01:00.000Z");
    expect(mergeOperationLogs([second], [first, second]).map((item) => item.id)).toEqual(["1", "2"]);
  });

  it("orders equal timestamps by ID and retains the newest 200 entries", () => {
    const timestamp = "2026-09-21T01:00:00.000Z";
    const older = Array.from({ length: 199 }, (_, index) => entry(`a${String(index).padStart(3, "0")}`, timestamp));
    const merged = mergeOperationLogs([entry("z", timestamp), ...older], [entry("y", timestamp), entry("x", timestamp)]);
    expect(merged).toHaveLength(200);
    expect(merged[0].id).toBe("a002");
    expect(merged[merged.length - 1]?.id).toBe("z");
  });

  it("returns no entries when the requested limit is zero", () => {
    expect(mergeOperationLogs([entry("1", "2026-09-21T01:00:00.000Z")], [], 0)).toEqual([]);
  });

  it("ignores malformed or unrelated plugin messages", () => {
    expect(parseOperationLogMessage("not JSON")).toBeNull();
    expect(parseOperationLogMessage(JSON.stringify({ message: "other target" }))).toBeNull();
    expect(parseOperationLogMessage(JSON.stringify({ ...entry("1", "invalid"), level: "debug" }))).toBeNull();
    expect(parseOperationLogMessage(JSON.stringify(entry("1", "2026-09-21T01:00:00.000Z")))).toEqual(
      entry("1", "2026-09-21T01:00:00.000Z"),
    );
  });
});
