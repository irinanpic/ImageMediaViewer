import { describe, expect, it } from "vitest";
import {
  DEFAULT_TIMELINE_FILTER,
  countPopoverFilterConditions,
  isTimelineFilterActive,
  serializeTimelineFilter,
} from "./filter";

describe("TimelineFilter utilities", () => {
  it("treats default filter as inactive and serializes to empty string", () => {
    expect(isTimelineFilterActive(DEFAULT_TIMELINE_FILTER)).toBe(false);
    expect(countPopoverFilterConditions(DEFAULT_TIMELINE_FILTER)).toBe(0);
    expect(serializeTimelineFilter(DEFAULT_TIMELINE_FILTER)).toBe("");
  });

  it("detects active conditions and counts popover filters accurately", () => {
    const f = {
      ...DEFAULT_TIMELINE_FILTER,
      query: "pose -draft",
      extensions: ["png", "clip"],
      excludeExtensions: ["gif"],
      aspectRatio: "landscape",
      includeTagIds: [1, 3],
      excludeTagIds: [2],
    };
    expect(isTimelineFilterActive(f)).toBe(true);
    // 2 inc formats + 1 exc format + 1 aspect ratio + 2 inc tags + 1 exc tag = 7
    expect(countPopoverFilterConditions(f)).toBe(7);
  });

  it("produces deterministic cache keys regardless of array order", () => {
    const f1 = {
      ...DEFAULT_TIMELINE_FILTER,
      extensions: ["webp", "png"],
      includeTagIds: [10, 2],
    };
    const f2 = {
      ...DEFAULT_TIMELINE_FILTER,
      extensions: ["png", "webp"],
      includeTagIds: [2, 10],
    };
    expect(serializeTimelineFilter(f1)).toBe(serializeTimelineFilter(f2));
  });
});
