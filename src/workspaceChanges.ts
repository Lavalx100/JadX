export type FileChange = {
  path: string;
  mode: string;
  added_lines: number;
  removed_lines: number;
  bytes_before: number;
  bytes_after: number;
  preview_title?: string;
  recovery_snapshot?: string;
  diff_preview?: string;
};

/** Merge incremental tool writes into the single review state for one run. */
export function mergeRunFileChanges(current: FileChange[], incoming: FileChange[]): FileChange[] {
  const byPath = new Map(current.map((change) => [change.path, change]));
  for (const next of incoming) {
    const previous = byPath.get(next.path);
    if (!previous) {
      byPath.set(next.path, next);
      continue;
    }
    const diffs = [previous.diff_preview, next.diff_preview].filter((value): value is string => Boolean(value));
    byPath.set(next.path, {
      ...next,
      mode: previous.mode === "created" ? previous.mode : next.mode,
      added_lines: previous.added_lines + next.added_lines,
      removed_lines: previous.removed_lines + next.removed_lines,
      bytes_before: previous.bytes_before,
      recovery_snapshot: previous.recovery_snapshot ?? next.recovery_snapshot,
      diff_preview: diffs.join("\n… later edit …\n").slice(0, 12_000),
    });
  }
  return [...byPath.values()];
}
