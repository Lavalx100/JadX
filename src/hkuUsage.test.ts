import { describe, expect, it } from "vitest";
import { HKU_QUOTA_GROUPS, observedHkuRequests } from "./hkuUsage";

describe("HKU published quota display", () => {
  it("shows Opus 4.6's published three-per-minute and 150-per-day limits", () => {
    const opus = HKU_QUOTA_GROUPS.find((group) => group.id === "claude")?.rules.find((rule) => rule.models[0] === "claude-opus-4.6");
    expect(opus).toMatchObject({ per_minute: 3, per_day: 150 });
  });

  it("aggregates shared provider caps separately from per-model caps", () => {
    const qwen = HKU_QUOTA_GROUPS.find((group) => group.id === "qwen-glm");
    const pool = qwen?.rules.find((rule) => rule.id === "qwen-glm:shared");
    const perModel = qwen?.rules.find((rule) => rule.models[0] === "qwen/qwen3-next-80b-a3b-instruct-maas");
    expect(pool?.per_day).toBe(500);
    expect(perModel).toMatchObject({ per_minute: 5, per_day: 300 });
    expect(observedHkuRequests(pool!, [
      { model: pool!.models[0], last_minute: 1, last_day: 4, last_week: 8, failed_last_day: 1 },
      { model: pool!.models[1], last_minute: 2, last_day: 3, last_week: 7, failed_last_day: 0 },
    ])).toEqual({ last_minute: 3, last_day: 7, last_week: 15, failed_last_day: 1 });
  });
});
