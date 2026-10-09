import { expect, it, vi } from "vitest";
import { prepareUserClose } from "./closeGuard";
it("confirms clean close only after acknowledged saves", async () => {
  const dependencies={flush:vi.fn(async()=>true),confirmDiscard:vi.fn(()=>false),exit:vi.fn(async()=>{})};
  expect(await prepareUserClose(dependencies)).toBe(true); expect(dependencies.exit).toHaveBeenCalledWith(true); expect(dependencies.confirmDiscard).not.toHaveBeenCalled();
});
it("blocks close when saving failed unless the user explicitly discards", async () => {
  const dependencies={flush:vi.fn(async()=>false),confirmDiscard:vi.fn(()=>false),exit:vi.fn(async()=>{})};
  expect(await prepareUserClose(dependencies)).toBe(false); expect(dependencies.exit).not.toHaveBeenCalled();
  dependencies.confirmDiscard.mockReturnValue(true); expect(await prepareUserClose(dependencies)).toBe(true); expect(dependencies.exit).toHaveBeenCalledWith(false);
});

it("offers the explicit discard decision rather than hanging forever on a stalled save", async () => {
  vi.useFakeTimers();
  try {
    const dependencies={flush:vi.fn(()=>new Promise<boolean>(()=>{})),confirmDiscard:vi.fn(()=>false),exit:vi.fn(async()=>{})};
    const closing=prepareUserClose(dependencies); await vi.advanceTimersByTimeAsync(5000);
    expect(await closing).toBe(false); expect(dependencies.confirmDiscard).toHaveBeenCalledOnce(); expect(dependencies.exit).not.toHaveBeenCalled();
  } finally { vi.useRealTimers(); }
});
