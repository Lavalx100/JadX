import { describe, expect, it } from "vitest";
import { CONTEXT_CANDIDATES, clampContext, profileDefaults } from "./harnessConfig";
import { dedupeConversationProjection } from "./conversationProjection";

describe("JadX feature configuration", () => {
  it("offers conservative-to-large context candidates", () => {
    expect(CONTEXT_CANDIDATES).toEqual([8192, 12288, 16384, 24576, 32768, 49152, 65536]);
  });

  it("clamps context to safe bounds and model maximum", () => {
    expect(clampContext(1000)).toBe(4096);
    expect(clampContext(20000, 16384)).toBe(16384);
    expect(clampContext(17000)).toBe(17408);
  });

  it("maps profiles to thinking defaults", () => {
    expect(profileDefaults("FAST")).toEqual({ thinkingMode: "Off", reasoningEffort: "Low" });
    expect(profileDefaults("DEEP")).toEqual({ thinkingMode: "On", reasoningEffort: "Medium" });
  });

  it("keeps the newest projection for repeated chat titles without deleting transcripts", () => {
    const visible = dedupeConversationProjection([
      { id: "old", title: "  Design the website  ", updated_at: "2026-09-20T10:00:00Z", pinned: false, archived: false },
      { id: "new", title: "Design the website", updated_at: "2026-09-21T10:00:00Z", pinned: false, archived: false },
      { id: "pinned", title: "Design the website", updated_at: "2026-09-19T10:00:00Z", pinned: true, archived: false },
      { id: "archived", title: "Design the website", updated_at: "2026-09-18T10:00:00Z", pinned: false, archived: true },
    ]);

    expect(visible.map((item) => item.id)).toEqual(["pinned", "archived"]);
  });

  it("does not merge same-title chats that belong to different workspaces", () => {
    const visible = dedupeConversationProjection([
      { id: "assistant", title: "Planning", updated_at: "2026-09-20T10:00:00Z", pinned: false, archived: false, workspace_id: null },
      { id: "project", title: "Planning", updated_at: "2026-09-19T10:00:00Z", pinned: false, archived: false, workspace_id: "workspace-1" },
      { id: "project-duplicate", title: "Planning", updated_at: "2026-09-18T10:00:00Z", pinned: false, archived: false, workspace_id: "workspace-1" },
    ]);

    expect(visible.map((item) => item.id)).toEqual(["assistant", "project"]);
  });

  it("keeps the currently open chat visible when repeated titles are collapsed", () => {
    const visible = dedupeConversationProjection([
      { id: "newer", title: "Planning", updated_at: "2026-09-22T10:00:00Z", pinned: false, archived: false },
      { id: "active", title: "Planning", updated_at: "2026-09-18T10:00:00Z", pinned: false, archived: false },
    ], "active");

    expect(visible.map((item) => item.id)).toEqual(["active"]);
  });
});
