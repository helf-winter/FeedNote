import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";

export type Page = "inbox" | "memories" | "memo" | "secrets" | "review" | "settings";

export interface FeedEvent {
  id: string;
  rawContent: string;
  sourceType: string;
  processingStatus: string;
  createdAt: number;
  memoryId?: string;
}

export interface MemorySummary {
  id: string;
  memoryType: string;
  lifecycleStatus: string;
  title: string;
  body: string;
  summary?: string;
  confidence: number;
  authorType: string;
  createdAt: number;
  updatedAt: number;
  sourceCount: number;
}

export interface MemoryVersion {
  id: string;
  title: string;
  body: string;
  summary?: string;
  confidence: number;
  authorType: string;
  modelInfo?: string;
  changeReason: string;
  createdAt: number;
  sourceEventIds: string[];
}

export interface MemoryDetail {
  memory: MemorySummary;
  versions: MemoryVersion[];
}

export interface ReviewItem {
  id: string;
  proposedAction: string;
  riskLevel: string;
  reason: string;
  status: string;
  payload: {
    memoryId?: string;
    feedId?: string;
    memoryType?: string;
    title?: string;
    summary?: string;
    confidence?: number;
    model?: string;
    question?: string;
  };
  createdAt: number;
}

export interface Stats {
  totalFeeds: number;
  totalMemories: number;
  pendingReviews: number;
  pendingProcessing: number;
}

export interface AppSettings {
  aiEnabled: boolean;
  llmEndpoint: string;
  llmModel: string;
  embeddingEndpoint: string;
  embeddingModel: string;
  embeddingDimensions: number;
  mobilePushEnabled: boolean;
  mobilePushProvider: "ntfy" | "webhook";
  mobileReminderMinutes: 0 | 5 | 15 | 30 | 60;
  feishuSyncEnabled: boolean;
  feishuTaskRemindersEnabled: boolean;
  feishuSourceEnabled: boolean;
  feishuSourceUrl: string;
  feishuSecretEnabled: boolean;
}

export interface VaultStatus {
  initialized: boolean;
  unlocked: boolean;
  secretCount: number;
}

export interface SecretItem {
  id: string;
  title: string;
  secretType: string;
  account?: string;
  secretValue: string;
  website?: string;
  notes?: string;
  sourceTitle: string;
  createdAt: number;
  updatedAt: number;
  feishuSyncedAt?: number;
}

export interface UpdateSecretInput {
  title: string;
  secretType: string;
  account?: string;
  secretValue: string;
  website?: string;
  notes?: string;
}

export interface SecretStashResult {
  secretId: string;
  message: string;
  undoUntil: number;
}

export interface MemoItem {
  id: string;
  content: string;
  sourceTitle: string;
  createdAt: number;
  feishuSyncedAt?: number;
}

export interface MemoCaptureResult {
  memoId: string;
  message: string;
}

export interface FeishuMemoStatus {
  configured: boolean;
  spreadsheetUrl?: string;
  pendingMemos: number;
  lastError?: string;
}

export interface FeishuSecretStatus {
  enabled: boolean;
  configured: boolean;
  spreadsheetUrl?: string;
  pendingSecrets: number;
  lastError?: string;
}

export interface FeishuSyncStatus {
  enabled: boolean;
  configured: boolean;
  spreadsheetUrl?: string;
  pendingPlans: number;
  lastError?: string;
  taskRemindersEnabled: boolean;
  pendingTaskReminders: number;
  taskReminderError?: string;
}

export interface FeishuSourceStatus {
  enabled: boolean;
  configured: boolean;
  spreadsheetUrl: string;
  sheetTitle?: string;
  totalRows: number;
  actionableRows: number;
  trackedRows: number;
  importedPlans: number;
  lastSyncAt?: number;
  lastError?: string;
}

export interface ProcessResult {
  status: string;
  message: string;
  reviewId?: string;
}

export interface SelectionSnapshot {
  selectedText: string;
  surroundingText: string;
  sourceTitle: string;
  capturedAt: number;
}

export interface PlanItem {
  id: string;
  feedEventId: string;
  title: string;
  details: string;
  content: string;
  linkUrl?: string;
  notes?: string;
  scheduledAt?: number;
  status: "scheduled" | "needs_clarification" | "done";
  clarificationQuestion?: string;
  sourceTitle: string;
  createdAt: number;
  updatedAt: number;
  remindedAt?: number;
}

export interface CaptureCommitResult {
  destination: "memory" | "plan" | "application" | "application_and_plan";
  message: string;
  plan?: PlanItem;
  applicationRecord?: {
    action: "created" | "updated";
    rowNumber: number;
    sheetTitle: string;
    company: string;
    role?: string;
  };
  needsClarification: boolean;
}

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const MOCK_KEY = "feednote-browser-preview-v1";

interface MockState {
  feeds: FeedEvent[];
  memories: MemorySummary[];
  reviews: ReviewItem[];
  settings: AppSettings;
}

function initialMockState(): MockState {
  const now = Date.now();
  return {
    feeds: [
      {
        id: "demo-feed-1",
        rawContent: "FeedNote 的原始输入必须永久可追溯，AI 只能创建新的理解版本。",
        sourceType: "manual",
        processingStatus: "classified",
        createdAt: now - 1000 * 60 * 18,
        memoryId: "demo-memory-1",
      },
      {
        id: "demo-feed-2",
        rawContent: "明天先把快捷输入和全文搜索跑通，再考虑系统右键菜单。",
        sourceType: "manual",
        processingStatus: "review",
        createdAt: now - 1000 * 60 * 74,
        memoryId: "demo-memory-2",
      },
    ],
    memories: [
      {
        id: "demo-memory-1",
        memoryType: "Decision",
        lifecycleStatus: "active",
        title: "记忆演进原则",
        body: "FeedNote 的原始输入必须永久可追溯，AI 只能创建新的理解版本。",
        summary: "原始输入不可改写，AI 理解通过新版本演进。",
        confidence: 0.96,
        authorType: "ai",
        createdAt: now - 1000 * 60 * 18,
        updatedAt: now - 1000 * 60 * 16,
        sourceCount: 1,
      },
      {
        id: "demo-memory-2",
        memoryType: "Unclassified",
        lifecycleStatus: "active",
        title: "明天先把快捷输入和全文搜索跑通...",
        body: "明天先把快捷输入和全文搜索跑通，再考虑系统右键菜单。",
        confidence: 1,
        authorType: "user",
        createdAt: now - 1000 * 60 * 74,
        updatedAt: now - 1000 * 60 * 74,
        sourceCount: 1,
      },
    ],
    reviews: [
      {
        id: "demo-review-1",
        proposedAction: "ask",
        riskLevel: "high",
        reason: "“明天”缺少可计算的具体日期，需要补充记录发生时间。",
        status: "pending",
        payload: {
          memoryId: "demo-memory-2",
          feedId: "demo-feed-2",
          memoryType: "Unclassified",
          title: "FeedNote 首轮开发顺序",
          summary: "先实现快捷输入和全文搜索，系统右键能力延后。",
          confidence: 0.46,
          model: "glm-5.3",
          question: "这里的“明天”具体是几月几日？",
        },
        createdAt: now - 1000 * 60 * 72,
      },
    ],
    settings: {
      aiEnabled: true,
      llmEndpoint: "https://open.bigmodel.cn/api/anthropic",
      llmModel: "glm-5.3",
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
    },
  };
}

function getMock(): MockState {
  const stored = localStorage.getItem(MOCK_KEY);
  if (!stored) {
    const state = initialMockState();
    localStorage.setItem(MOCK_KEY, JSON.stringify(state));
    return state;
  }
  const state = JSON.parse(stored) as MockState;
  state.settings = { ...initialMockState().settings, ...state.settings };
  return state;
}

function setMock(state: MockState): void {
  localStorage.setItem(MOCK_KEY, JSON.stringify(state));
}

function id(prefix: string): string {
  return `${prefix}-${Date.now()}-${Math.random().toString(16).slice(2)}`;
}

function titleFrom(content: string): string {
  const first = content.trim().split(/\r?\n/)[0] ?? content;
  return first.length > 36 ? `${first.slice(0, 36)}...` : first;
}

export async function createFeed(content: string): Promise<{ feedId: string; memoryId: string }> {
  if (isTauri) {
    return invoke("create_feed", { input: { content, sourceType: "manual" } });
  }
  const state = getMock();
  const feedId = id("feed");
  const memoryId = id("memory");
  const createdAt = Date.now();
  state.feeds.unshift({
    id: feedId,
    rawContent: content.trim(),
    sourceType: "manual",
    processingStatus: "pending",
    createdAt,
    memoryId,
  });
  state.memories.unshift({
    id: memoryId,
    memoryType: "Unclassified",
    lifecycleStatus: "active",
    title: titleFrom(content),
    body: content.trim(),
    confidence: 1,
    authorType: "user",
    createdAt,
    updatedAt: createdAt,
    sourceCount: 1,
  });
  setMock(state);
  return { feedId, memoryId };
}

export async function listFeeds(query = ""): Promise<FeedEvent[]> {
  if (isTauri) return invoke("list_feeds", { query: query || null, limit: 200 });
  return getMock().feeds.filter((feed) => feed.rawContent.toLowerCase().includes(query.toLowerCase()));
}

export async function listMemories(query = "", memoryType = ""): Promise<MemorySummary[]> {
  if (isTauri) {
    return invoke("list_memories", {
      query: query || null,
      memoryType: memoryType || null,
      limit: 300,
    });
  }
  return getMock().memories.filter((memory) => {
    const matchesQuery = `${memory.title} ${memory.body} ${memory.summary ?? ""}`
      .toLowerCase()
      .includes(query.toLowerCase());
    return matchesQuery && (!memoryType || memory.memoryType === memoryType);
  });
}

export async function getMemory(memoryId: string): Promise<MemoryDetail> {
  if (isTauri) return invoke("get_memory", { memoryId });
  const memory = getMock().memories.find((item) => item.id === memoryId);
  if (!memory) throw new Error("记忆不存在");
  return {
    memory,
    versions: [
      {
        id: `${memory.id}-current`,
        title: memory.title,
        body: memory.body,
        summary: memory.summary,
        confidence: memory.confidence,
        authorType: memory.authorType,
        modelInfo: memory.authorType === "ai" ? "glm-5.3" : undefined,
        changeReason: memory.authorType === "ai" ? "AI 自动分类与整理" : "原始投喂",
        createdAt: memory.updatedAt,
        sourceEventIds: getMock().feeds.filter((feed) => feed.memoryId === memory.id).map((feed) => feed.id),
      },
    ],
  };
}

export async function listReviews(): Promise<ReviewItem[]> {
  if (isTauri) return invoke("list_reviews");
  return getMock().reviews.filter((review) => review.status === "pending");
}

export async function resolveReview(reviewId: string, accept: boolean): Promise<void> {
  if (isTauri) return invoke("resolve_review", { reviewId, accept });
  const state = getMock();
  const review = state.reviews.find((item) => item.id === reviewId);
  if (!review) throw new Error("待澄清项不存在");
  review.status = accept ? "accepted" : "rejected";
  const feed = state.feeds.find((item) => item.id === review.payload.feedId);
  if (feed) feed.processingStatus = review.status;
  if (accept && review.proposedAction !== "ask" && review.payload.memoryId) {
    const memory = state.memories.find((item) => item.id === review.payload.memoryId);
    if (memory) {
      memory.memoryType = review.payload.memoryType ?? memory.memoryType;
      memory.title = review.payload.title ?? memory.title;
      memory.summary = review.payload.summary;
      memory.confidence = review.payload.confidence ?? memory.confidence;
      memory.authorType = "ai";
      memory.updatedAt = Date.now();
    }
  }
  setMock(state);
}

export async function requestDeleteFeed(feedId: string): Promise<string> {
  if (isTauri) {
    const confirmation = await invoke<{ token: string; expiresAt: number }>("request_delete_feed", { feedId });
    return confirmation.token;
  }
  return `preview-confirm-${feedId}`;
}

export async function deleteFeed(feedId: string, confirmationToken: string): Promise<void> {
  if (isTauri) return invoke("delete_feed", { feedId, confirmationToken });
  if (confirmationToken !== `preview-confirm-${feedId}`) throw new Error("删除确认已失效");
  const state = getMock();
  const feed = state.feeds.find((item) => item.id === feedId);
  state.feeds = state.feeds.filter((item) => item.id !== feedId);
  if (feed?.memoryId) state.memories = state.memories.filter((item) => item.id !== feed.memoryId);
  state.reviews = state.reviews.filter((item) => item.payload.feedId !== feedId);
  setMock(state);
}

export async function getStats(): Promise<Stats> {
  if (isTauri) return invoke("get_stats");
  const state = getMock();
  return {
    totalFeeds: state.feeds.length,
    totalMemories: state.memories.length,
    pendingReviews: state.reviews.filter((item) => item.status === "pending").length,
    pendingProcessing: state.feeds.filter((item) => ["pending", "processing"].includes(item.processingStatus)).length,
  };
}

export async function getSettings(): Promise<AppSettings> {
  if (isTauri) return invoke("get_settings");
  return getMock().settings;
}

export async function updateSettings(settings: AppSettings): Promise<void> {
  if (isTauri) return invoke("update_settings", { input: settings });
  const llmEndpoint = settings.llmEndpoint.replace(/\/$/, "");
  const embeddingEndpoint = settings.embeddingEndpoint.replace(/\/$/, "");
  const local = /^http:\/\/(127\.0\.0\.1|localhost)(:\d+)?$/;
  if (llmEndpoint !== "https://open.bigmodel.cn/api/anthropic" && !local.test(llmEndpoint)) {
    throw new Error("只允许连接已授权的智谱 Anthropic 地址或本机模型服务");
  }
  if (embeddingEndpoint !== "https://open.bigmodel.cn/api/paas/v4" && !local.test(embeddingEndpoint)) {
    throw new Error("只允许连接已授权的智谱 Embedding 地址或本机服务");
  }
  if (settings.feishuSourceEnabled || settings.feishuSourceUrl) {
    const source = new URL(settings.feishuSourceUrl);
    const allowedHost = source.hostname === "feishu.cn" || source.hostname.endsWith(".feishu.cn")
      || source.hostname === "larksuite.com" || source.hostname.endsWith(".larksuite.com");
    if (source.protocol !== "https:" || !allowedHost || !source.pathname.includes("/sheets/")) {
      throw new Error("飞书来源只允许 feishu.cn 或 larksuite.com 的 /sheets/ HTTPS 链接");
    }
  }
  const state = getMock();
  state.settings = settings;
  setMock(state);
}

export async function checkAi(): Promise<string> {
  if (isTauri) return invoke("check_ai");
  throw new Error("浏览器预览不会读取本机密钥，请在桌面应用中测试");
}

export async function processFeed(feedId: string): Promise<ProcessResult> {
  if (isTauri) return invoke("process_feed", { feedId });
  const state = getMock();
  const settings = state.settings;
  if (settings.aiEnabled) {
    const feed = state.feeds.find((item) => item.id === feedId);
    const memory = state.memories.find((item) => item.id === feed?.memoryId);
    if (feed && memory) {
      const taskLike = /要|需要|完成|记得|明天|周[一二三四五六日]/.test(feed.rawContent);
      memory.memoryType = taskLike ? "Task" : "Knowledge";
      memory.title = titleFrom(feed.rawContent);
      memory.summary = feed.rawContent;
      memory.confidence = 0.82;
      memory.authorType = "ai";
      memory.updatedAt = Date.now();
      feed.processingStatus = "classified";
      setMock(state);
    }
  }
  return {
    status: settings.aiEnabled ? "classified" : "disabled",
    message: settings.aiEnabled ? "已自动理解并归入记忆" : "AI 整理已关闭，原始记录已安全保存",
  };
}

export async function prepareCapture(): Promise<SelectionSnapshot> {
  return invoke("prepare_capture");
}

export async function prepareDragCapture(text: string): Promise<SelectionSnapshot> {
  return invoke("prepare_drag_capture", { text });
}

export async function getCapturePreview(): Promise<SelectionSnapshot | null> {
  return invoke("get_capture_preview");
}

export async function recordMemoCapture(): Promise<MemoCaptureResult> {
  return invoke("record_memo_capture");
}

export async function listMemos(limit = 500): Promise<MemoItem[]> {
  if (isTauri) return invoke("list_memos", { limit });
  return [];
}

export async function discardCapture(): Promise<void> {
  return invoke("discard_capture");
}

export async function commitCapture(): Promise<CaptureCommitResult> {
  return invoke("commit_capture");
}

export async function getVaultStatus(): Promise<VaultStatus> {
  if (isTauri) return invoke("get_vault_status");
  return { initialized: false, unlocked: false, secretCount: 0 };
}

export async function initializeVault(password: string): Promise<VaultStatus> {
  if (isTauri) return invoke("initialize_vault", { password });
  return { initialized: true, unlocked: true, secretCount: 0 };
}

export async function unlockVault(password: string): Promise<VaultStatus> {
  if (isTauri) return invoke("unlock_vault", { password });
  return { initialized: true, unlocked: true, secretCount: 0 };
}

export async function lockVault(): Promise<VaultStatus> {
  if (isTauri) return invoke("lock_vault");
  return { initialized: true, unlocked: false, secretCount: 0 };
}

export async function listSecretItems(): Promise<SecretItem[]> {
  if (isTauri) return invoke("list_secret_items");
  return [];
}

export async function updateSecretItem(secretId: string, input: UpdateSecretInput): Promise<SecretItem> {
  if (isTauri) return invoke("update_secret_item", { secretId, input });
  return { id: secretId, sourceTitle: "", createdAt: Date.now(), updatedAt: Date.now(), ...input };
}

export async function deleteSecretItem(secretId: string, password: string): Promise<void> {
  if (isTauri) return invoke("delete_secret_item", { secretId, password });
}

export async function stashCapture(): Promise<SecretStashResult> {
  return invoke("stash_capture");
}

export async function undoSecretStash(secretId: string): Promise<void> {
  return invoke("undo_secret_stash", { secretId });
}

export async function resolvePlanTime(planId: string, answer: string): Promise<CaptureCommitResult> {
  return invoke("resolve_plan_time", { planId, answer });
}

export async function listPlans(includeDone = false): Promise<PlanItem[]> {
  return invoke("list_plans", { includeDone });
}

export async function setPlanDone(planId: string, done: boolean): Promise<PlanItem> {
  return invoke("set_plan_done", { planId, done });
}

export async function togglePlanDock(): Promise<boolean> {
  return invoke("toggle_plan_dock");
}

export async function openMainWindow(): Promise<void> {
  return invoke("open_main_window");
}

export async function openExternalLink(url: string): Promise<void> {
  if (isTauri) return invoke("open_external_link", { url });
  window.open(url, "_blank", "noopener,noreferrer");
}

export async function testMobilePush(provider: AppSettings["mobilePushProvider"]): Promise<string> {
  if (isTauri) return invoke("test_mobile_push", { provider });
  throw new Error("浏览器预览不会读取本机推送密钥，请在桌面应用中测试");
}

export async function getFeishuSyncStatus(): Promise<FeishuSyncStatus> {
  if (isTauri) return invoke("get_feishu_sync_status");
  return {
    enabled: false,
    configured: false,
    pendingPlans: 0,
    taskRemindersEnabled: false,
    pendingTaskReminders: 0,
  };
}

export async function syncFeishuNow(): Promise<string> {
  if (isTauri) return invoke("sync_feishu_now");
  throw new Error("浏览器预览不会读取飞书应用凭证，请在桌面应用中同步");
}

export async function getFeishuSecretStatus(): Promise<FeishuSecretStatus> {
  if (isTauri) return invoke("get_feishu_secret_status");
  return {
    enabled: false,
    configured: false,
    pendingSecrets: 0,
  };
}

export async function syncFeishuSecretsNow(): Promise<string> {
  if (isTauri) return invoke("sync_feishu_secrets_now");
  throw new Error("浏览器预览不会读取飞书应用凭证，请在桌面应用中同步");
}

export async function getFeishuMemoStatus(): Promise<FeishuMemoStatus> {
  if (isTauri) return invoke("get_feishu_memo_status");
  return { configured: false, pendingMemos: 0 };
}

export async function syncFeishuMemosNow(): Promise<string> {
  if (isTauri) return invoke("sync_feishu_memos_now");
  throw new Error("浏览器预览不会读取飞书应用凭证，请在桌面应用中同步");
}

export async function getFeishuSourceStatus(): Promise<FeishuSourceStatus> {
  if (isTauri) return invoke("get_feishu_source_status");
  return {
    enabled: false,
    configured: false,
    spreadsheetUrl: "",
    totalRows: 0,
    actionableRows: 0,
    trackedRows: 0,
    importedPlans: 0,
  };
}

export async function syncFeishuSourceNow(): Promise<string> {
  if (isTauri) return invoke("sync_feishu_source_now");
  throw new Error("浏览器预览不会读取飞书应用凭证，请在桌面应用中分析");
}

export async function exportArchive(): Promise<boolean> {
  if (isTauri) {
    const path = await save({
      title: "导出 FeedNote 数据",
      defaultPath: `feednote-export-${new Date().toISOString().slice(0, 10)}.json`,
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (!path) return false;
    await invoke("export_data", { path });
    return true;
  }
  const state = getMock();
  const blob = new Blob(
    [JSON.stringify({ schemaVersion: 1, exportedAt: new Date().toISOString(), ...state }, null, 2)],
    { type: "application/json" },
  );
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = `feednote-export-${new Date().toISOString().slice(0, 10)}.json`;
  anchor.click();
  URL.revokeObjectURL(url);
  return true;
}

export { isTauri };
