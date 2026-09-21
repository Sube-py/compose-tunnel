import { describe, expect, it } from "vitest";
import { decodeOutput, isCommandLogPersistenceError, mergeCommandLogs, parseCommandLogMessage, type CommandLogEntry } from "./command-logs";

const row = (id: string, revision = 0): CommandLogEntry => ({
  id, revision, started_at: "2026-09-21T01:00:00.000Z", updated_at: "2026-09-21T01:00:00.000Z",
  server: "staging", preview: "docker ps", status: revision ? "success" : "running",
  exit_code: revision ? 0 : null, duration_ms: revision ? 10 : null, detail_available: true,
});

describe("command logs", () => {
  it("does not regress a live revision when older history arrives", () => {
    expect(mergeCommandLogs([row("a", 1)], [row("a")])).toEqual([row("a", 1)]);
  });

  it("retains newest 200 distinct commands", () => {
    const rows = Array.from({ length: 202 }, (_, index) => row(`command-${String(index).padStart(3, "0")}`));
    expect(mergeCommandLogs([], rows)).toHaveLength(200);
    expect(mergeCommandLogs([], rows)[0]?.id).toBe("command-201");
    expect(mergeCommandLogs([], rows)[199]?.id).toBe("command-002");
  });

  it("ignores malformed and unrelated plugin messages", () => {
    expect(parseCommandLogMessage("not JSON")).toBeNull();
    expect(parseCommandLogMessage(JSON.stringify({ operation: "Docker inspect" }))).toBeNull();
    expect(parseCommandLogMessage(JSON.stringify({ ...row("a"), status: "unknown" }))).toBeNull();
    expect(parseCommandLogMessage(JSON.stringify(row("a")))).toEqual(row("a"));
  });

  it("recognizes a safe persistence warning without treating it as a command", () => {
    const warning = JSON.stringify({ kind: "persistence_error" });
    expect(isCommandLogPersistenceError(warning)).toBe(true);
    expect(parseCommandLogMessage(warning)).toBeNull();
    expect(isCommandLogPersistenceError("not JSON")).toBe(false);
  });

  it("shows exact UTF-8 text and visibly escapes invalid bytes", () => {
    expect(decodeOutput("5L2g5aW9")).toBe("你好");
    expect(decodeOutput("//4=")).toBe("\\xFF\\xFE");
  });
});
