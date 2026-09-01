import { beforeEach, describe, expect, it } from "vitest";
import {
  createFeed,
  deleteFeed,
  getSettings,
  getMemory,
  getStats,
  listFeeds,
  listMemories,
  processFeed,
  requestDeleteFeed,
  updateSettings,
} from "./api";

describe("browser preview data adapter", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("keeps the original feed as the source of a new memory", async () => {
    const created = await createFeed("原始记录不能被 AI 改写");
    const feed = (await listFeeds("原始记录"))[0];
    const memory = await getMemory(created.memoryId);

    expect(feed.rawContent).toBe("原始记录不能被 AI 改写");
    expect(memory.versions[0].body).toBe(feed.rawContent);
    expect(memory.versions[0].authorType).toBe("user");
  });

  it("permanent deletion removes the feed and its derived memory", async () => {
    const created = await createFeed("这条稍后删除");
    const token = await requestDeleteFeed(created.feedId);
    await deleteFeed(created.feedId, token);

    expect(await listFeeds("这条稍后删除")).toHaveLength(0);
    expect(await listMemories("这条稍后删除")).toHaveLength(0);
  });

  it("refuses permanent deletion without a confirmation token", async () => {
    const created = await createFeed("不能绕过确认");
    await expect(deleteFeed(created.feedId, "invalid-token")).rejects.toThrow("确认已失效");
  });

  it("refuses an unauthorized model endpoint", async () => {
    await expect(
      updateSettings({
        aiEnabled: true,
        launchAtLogin: false,
        llmEndpoint: "https://example.com",
        llmModel: "deepseek-v4-flash",
        embeddingEndpoint: "https://open.bigmodel.cn/api/paas/v4",
        embeddingModel: "embedding-3",
        embeddingDimensions: 512,
        mobilePushEnabled: false,
        mobilePushProvider: "ntfy",
        mobileReminderMinutes: 15,
        feishuSyncEnabled: false,
        feishuTaskRemindersEnabled: false,
        feishuSourceEnabled: false,
        feishuSourceUrl: "",
        feishuSecretEnabled: false,
      }),
    ).rejects.toThrow("只允许连接已授权");
  });

  it("persists the launch-at-login preference", async () => {
    const settings = await getSettings();
    await updateSettings({ ...settings, launchAtLogin: true });
    expect((await getSettings()).launchAtLogin).toBe(true);
  });

  it("updates statistics after capture", async () => {
    const before = await getStats();
    await createFeed("统计测试");
    const after = await getStats();
    expect(after.totalFeeds).toBe(before.totalFeeds + 1);
    expect(after.totalMemories).toBe(before.totalMemories + 1);
  });

  it("classifies a feed automatically without creating a suggestion", async () => {
    const created = await createFeed("周五前完成搜索功能");
    const result = await processFeed(created.feedId);
    const memory = await getMemory(created.memoryId);

    expect(result.status).toBe("classified");
    expect(memory.memory.memoryType).toBe("Task");
    expect(memory.memory.authorType).toBe("ai");
  });
});
