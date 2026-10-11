import { describe, expect, it, vi } from "vitest";
import { syncPlansFromFeishu } from "./planDockSync";

describe("syncPlansFromFeishu", () => {
  it("refreshes the desktop plans after a successful manual sync", async () => {
    const sync = vi.fn().mockResolvedValue("已同步 3 条飞书待办提醒");
    const refresh = vi.fn().mockResolvedValue(undefined);

    await expect(syncPlansFromFeishu(sync, refresh)).resolves.toBe(
      "已同步 3 条飞书待办提醒",
    );
    expect(sync).toHaveBeenCalledOnce();
    expect(refresh).toHaveBeenCalledOnce();
    expect(sync.mock.invocationCallOrder[0]).toBeLessThan(
      refresh.mock.invocationCallOrder[0],
    );
  });

  it("does not refresh plans when the manual sync fails", async () => {
    const sync = vi.fn().mockRejectedValue(new Error("飞书同步失败"));
    const refresh = vi.fn().mockResolvedValue(undefined);

    await expect(syncPlansFromFeishu(sync, refresh)).rejects.toThrow(
      "飞书同步失败",
    );
    expect(refresh).not.toHaveBeenCalled();
  });
});
