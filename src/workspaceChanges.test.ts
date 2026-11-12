import { describe, expect, it } from "vitest";
import { mergeRunFileChanges, type FileChange } from "./workspaceChanges";

const change = (overrides: Partial<FileChange> = {}): FileChange => ({
  path: "src/notes.ts",
  mode: "modified",
  added_lines: 2,
  removed_lines: 1,
  bytes_before: 100,
  bytes_after: 120,
  recovery_snapshot: "original-snapshot",
  diff_preview: "-old\n+new",
  ...overrides,
});

describe("mergeRunFileChanges", () => {
  it("keeps one review entry per path and preserves the first recovery point", () => {
    const merged = mergeRunFileChanges([change()], [change({
      added_lines: 1,
      removed_lines: 2,
      bytes_before: 120,
      bytes_after: 110,
      recovery_snapshot: "later-snapshot",
      diff_preview: "-new\n+final",
    })]);

    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({
      bytes_before: 100,
      bytes_after: 110,
      added_lines: 3,
      removed_lines: 3,
      recovery_snapshot: "original-snapshot",
      diff_preview: expect.stringContaining("later edit"),
    });
  });

  it("retains created status and includes newly touched files", () => {
    const merged = mergeRunFileChanges(
      [change({ mode: "created", recovery_snapshot: undefined })],
      [change({ path: "src/store.ts", mode: "created" })],
    );
    expect(merged.map((file) => file.path)).toEqual(["src/notes.ts", "src/store.ts"]);
    expect(merged[0].mode).toBe("created");
  });
});
