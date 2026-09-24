import { describe, expect, it } from "vitest";
import { createScheduledTask, normalizeSchedulerState } from "../defaults";
import { entryToUi, joinArguments, localInputToIso, splitArguments, uiToPatch, uiToSpec, type ScheduleEntryV9 } from "../scheduleAdapter";
import { calendarCron, nextCalendarInstant, nextRunPreview, readCalendar, scheduleDirty, validateScheduleDraft, visibleScheduleHistory } from "../scheduleEditor";
import type { ScheduleHistoryRun } from "../types";

function entry(trigger: any = { type: "calendar", cron: "15 8 * * MON,WED", timezone: "America/New_York", missedPolicy: "skip" }): ScheduleEntryV9 {
  return {
    id: "schedule-test", spec: {
      name: "test", enabled: true, trigger, until: "2035-12-31",
      gate: { windows: [{ start: "22:00", end: "02:00", weekdays: [1, 5] }],
        probe: { type: "script", language: "python", source: { type: "file", path: "C:\\tools\\probe.py" }, args: ["", "C:\\folder name\\", "quoted\"text"], timeout: "1250ms", workingDirectory: "C:\\tools" },
        when: { stream: "stderr", mode: "regex", pattern: "^READY" } },
      action: { type: "script", language: "python", source: { type: "inline", code: "print('ok')" }, timeout: "1500ms",
        args: ["", "C:\\data", "it's", "a b"], notifications: {
          onComplete: { message: "{stdout}", duration: "5s", position: "top-left" },
          onOutput: { when: { stream: "stderr", mode: "contains", pattern: "ready" }, notification: { message: "{stderr}", duration: "3s" } }
        } }
    }, state: { runCount: 2, lastStatus: "success", nextRunAt: "2030-01-01T12:00:00Z" }, ui: { expanded: true, editing: true }, createdAt: "2029-01-01T00:00:00Z", updatedAt: "2029-01-01T00:00:00Z"
  };
}

describe("scheduler adapter preserves source semantics", () => {
  it("round-trips without touching timezone, timeouts, streams or advanced fields", () => {
    const source = entry();
    const ui = entryToUi(source);
    expect(uiToPatch(ui, source)).toEqual({});
    expect(ui.trigger.timezone).toBe("America/New_York");
    expect(ui.action.timeout).toBe("1500ms");
    expect(ui.gate?.condition?.stream).toBe("stderr");
    expect(uiToPatch({ ...ui, name: "renamed" }, source)).toEqual({ name: "renamed" });
    expect(uiToPatch({ ...ui, action: { ...ui.action, code: "print('changed')" } }, source))
      .toEqual({ action: { source: { code: "print('changed')" } } });
  });
  it("preserves a once instant's offset and subminute precision on unrelated edits", () => {
    const source = entry({ type: "once", at: "2030-05-06T07:08:59.123+08:00", missedPolicy: "run-once" });
    expect(uiToPatch({ ...entryToUi(source), name: "new" }, source)).toEqual({ name: "new" });
  });
  it("retains subsecond polling, probe options and clears optional settings explicitly", () => {
    const source = entry({ type: "condition", every: "250ms", cooldown: "30s", probe: entry().spec.gate.probe,
      when: { stream: "stdout", pattern: "READY" }, missedPolicy: "run-once" });
    delete source.spec.gate;
    const ui = entryToUi(source);
    expect(ui.trigger.everySeconds).toBe(0.25);
    expect(normalizeSchedulerState({ tasks: [ui] }).tasks[0].trigger.everySeconds).toBe(0.25);
    expect(uiToPatch(ui, source)).toEqual({});
    expect(uiToPatch({ ...ui, until: undefined, trigger: { ...ui.trigger, cooldown: undefined } }, source))
      .toEqual({ until: null, trigger: { cooldown: null } });
  });
  it("removes gate and nested notification settings without discarding other fields", () => {
    const source = entry(), ui = entryToUi(source);
    expect(uiToPatch({ ...ui, gate: undefined, action: { ...ui.action, notifyOnComplete: false } }, source))
      .toEqual({ gate: null, action: { notifications: { onComplete: null } } });
    const patch = uiToPatch({ ...ui, gate: { ...ui.gate!, probeAction: { ...ui.gate!.probeAction!, code: "unused", timeout: "2s" } } }, source);
    expect(patch).toEqual({ gate: { probe: { timeout: "2s" } } });
  });
  it("keeps new scheduler fields through browser persistence normalization", () => {
    const source = entryToUi(entry());
    const normalized = normalizeSchedulerState({ tasks: [source] }).tasks[0];
    expect(normalized.gate).toEqual(source.gate);
    expect(normalized.until).toBe(source.until);
    expect(normalized.trigger.timezone).toBe(source.trigger.timezone);
    expect(normalized.action.timeout).toBe(source.action.timeout);
    expect(normalized.editing).toBe(true);
    expect(normalizeSchedulerState({ tasks: [normalized] }).tasks[0]).toEqual(normalized);
  });
  it("round-trips empty, escaped and Windows arguments", () => {
    const args = ["", "C:\\tools\\file.py", "C:\\folder name\\", "it's", 'say "hello"', "one two"];
    expect(splitArguments(joinArguments(args))).toEqual(args);
    expect(splitArguments("C:\\tools\\file.py")).toEqual(["C:\\tools\\file.py"]);
    expect(() => splitArguments('"unfinished')).toThrow();
  });
  it("never silently converts invalid local input to now", () => {
    for (const bad of ["", "invalid", "2030-02-30T09:00", "2030-01-01T25:00"]) expect(() => localInputToIso(bad)).toThrow();
  });
});

describe("scheduler guided editor", () => {
  it("builds daily, weekly and monthly plans and leaves advanced cron alone", () => {
    expect(calendarCron({ mode: "weekly", time: "08:30", weekdays: [1, 3, 5], day: 1 })).toBe("30 8 * * MON,WED,FRI");
    expect(readCalendar("30 8 * * MON,WED,FRI")).toMatchObject({ mode: "weekly", weekdays: [1, 3, 5] });
    expect(readCalendar("0 9 31 * *")).toMatchObject({ mode: "monthly", day: 31 });
    expect(readCalendar("*/10 8-18 * * 1-5").mode).toBe("advanced");
    expect(readCalendar("0 0 9 * * MON *").mode).toBe("advanced");
    expect(() => calendarCron({ mode: "weekly", time: "09:00", weekdays: [], day: 1 })).toThrow();
  });
  it("validates gates, mobile restrictions, cooldown and action disclosures", () => {
    const task = entryToUi(entry());
    expect(validateScheduleDraft(task, true)).toBeNull();
    expect(validateScheduleDraft(task, false)).not.toBeNull();
    for (const patch of [
      { until: "2030-02-30" }, { gate: { windows: [] } },
      { gate: { windows: [{ start: "24:00", end: "02:00" }] } },
      { gate: { windows: [{ start: "09:00", end: "18:00", weekdays: [] }] } },
      { gate: { windows: [{ start: "09:00", end: "09:00" }] } }
    ]) expect(validateScheduleDraft({ ...task, ...patch }, true)).not.toBeNull();
    expect(validateScheduleDraft({ ...task, action: { ...task.action, timeout: "0s" } }, true)).not.toBeNull();
    const mobile = createScheduledTask(); mobile.action.type = "notification";
    mobile.gate = { windows: [{ start: "09:00", end: "18:00" }] };
    expect(validateScheduleDraft(mobile, false)).toBeNull();
    const conditional = { ...task, gate: undefined, trigger: { ...task.trigger, type: "condition" as const, cooldown: "0s" } };
    expect(validateScheduleDraft(conditional, true)).not.toBeNull();
  });
  it("previews in schedule timezone and honors inclusive until", () => {
    const task = entryToUi(entry()); task.gate = undefined;
    task.trigger.cron = "0 9 * * *"; task.trigger.timezone = "Asia/Shanghai"; task.until = "2030-01-01";
    expect(nextRunPreview(task, new Date("2030-01-01T00:00:00Z"))).toContain("Asia/Shanghai");
    expect(nextRunPreview(task, new Date("2030-01-01T02:00:00Z"))).toBe("截止日期内无下次触发");
    expect(nextRunPreview(task, new Date("2030-01-01T16:00:00Z"))).toBe("已过截止日期");
  });
  it("tracks actual spec changes, not expansion or runtime state", () => {
    const task = createScheduledTask();
    expect(scheduleDirty({ ...task, expanded: false, nextRunAt: undefined, runCount: 3 }, task)).toBe(false);
    expect(scheduleDirty({ ...task, enabled: !task.enabled }, task)).toBe(true);
    expect(uiToSpec(task)).not.toHaveProperty("state");
  });
  it("filters only ordinary probe nonmatches, retaining failures and sorting by instants", () => {
    const run = (patch: Partial<ScheduleHistoryRun>): ScheduleHistoryRun => ({ taskId: "test", kind: "scheduled", status: "success", startedAt: "2030-01-01T00:00:00Z", finishedAt: "2030-01-01T00:00:00Z", ...patch });
    const failure = run({ kind: "probe", status: "failed", exitCode: 7, stopReason: "probe 未命中" });
    const newer = run({ kind: "manual", startedAt: "2030-01-01T09:00:00+08:00" });
    expect(visibleScheduleHistory([newer, failure, run({ kind: "probe", exitCode: 0, stopReason: "probe 未命中" })])).toEqual([newer, failure]);
  });
});
