import {
  createAsyncThunk,
  createSlice,
  type PayloadAction,
} from "@reduxjs/toolkit";
import {
  getFeishuMemoStatus,
  getFeishuSecretStatus,
  getFeishuSyncStatus,
  getSettings,
  getStats,
  getVaultStatus,
  listFeeds,
  listMemories,
  listMemos,
  listPlans,
  listReviews,
  type AppSettings,
  type FeedEvent,
  type FeishuMemoStatus,
  type FeishuSecretStatus,
  type FeishuSyncStatus,
  type MemorySummary,
  type MemoItem,
  type PlanItem,
  type ReviewItem,
  type Stats,
  type VaultStatus,
} from "../api";

export const defaultSettings: AppSettings = {
  launchAtLogin: false,
  aiEnabled: true,
  llmEndpoint: "https://api.deepseek.com/anthropic",
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
};

interface AppState {
  feeds: FeedEvent[];
  memories: MemorySummary[];
  memos: MemoItem[];
  plans: PlanItem[];
  reviews: ReviewItem[];
  stats: Stats;
  settings: AppSettings;
  vault: VaultStatus;
  feishu: FeishuSyncStatus;
  feishuMemo: FeishuMemoStatus;
  feishuSecret: FeishuSecretStatus;
  loading: boolean;
  error?: string;
}

const initialState: AppState = {
  feeds: [],
  memories: [],
  memos: [],
  plans: [],
  reviews: [],
  stats: {
    totalFeeds: 0,
    totalMemories: 0,
    pendingReviews: 0,
    pendingProcessing: 0,
  },
  settings: defaultSettings,
  vault: { initialized: false, unlocked: false, secretCount: 0 },
  feishu: {
    enabled: false,
    configured: false,
    pendingPlans: 0,
    taskRemindersEnabled: false,
    pendingTaskReminders: 0,
  },
  feishuMemo: { configured: false, pendingMemos: 0 },
  feishuSecret: { enabled: false, configured: false, pendingSecrets: 0 },
  loading: true,
};

export const loadDashboard = createAsyncThunk(
  "app/loadDashboard",
  async (filters?: { feed?: string; memory?: string; type?: string }) => {
    const [feeds, memories, reviews, stats, settings, plans] =
      await Promise.all([
        listFeeds(filters?.feed ?? ""),
        listMemories(filters?.memory ?? "", filters?.type ?? ""),
        listReviews(),
        getStats(),
        getSettings(),
        listPlans(true),
      ]);
    return { feeds, memories, reviews, stats, settings, plans };
  },
);

export const loadFeeds = createAsyncThunk("app/loadFeeds", (query: string) =>
  listFeeds(query),
);
export const loadMemories = createAsyncThunk(
  "app/loadMemories",
  (filter: { query: string; type: string }) =>
    listMemories(filter.query, filter.type),
);
export const loadPlans = createAsyncThunk(
  "app/loadPlans",
  (includeDone: boolean = true) => listPlans(includeDone),
);
export const loadMemos = createAsyncThunk("app/loadMemos", async () => {
  const [memos, status] = await Promise.all([
    listMemos(),
    getFeishuMemoStatus(),
  ]);
  return { memos, status };
});
export const loadFeishuStatuses = createAsyncThunk(
  "app/loadFeishuStatuses",
  async () => {
    const [feishu, feishuMemo, feishuSecret] = await Promise.all([
      getFeishuSyncStatus(),
      getFeishuMemoStatus(),
      getFeishuSecretStatus(),
    ]);
    return { feishu, feishuMemo, feishuSecret };
  },
);
export const loadVault = createAsyncThunk("app/loadVault", async () => {
  return getVaultStatus();
});

const appSlice = createSlice({
  name: "app",
  initialState,
  reducers: {
    settingsChanged(state, action: PayloadAction<AppSettings>) {
      state.settings = action.payload;
    },
    vaultChanged(state, action: PayloadAction<VaultStatus>) {
      state.vault = action.payload;
    },
    clearError(state) {
      state.error = undefined;
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(loadDashboard.pending, (state) => {
        state.loading = true;
        state.error = undefined;
      })
      .addCase(loadDashboard.fulfilled, (state, { payload }) => {
        Object.assign(state, payload);
        state.loading = false;
      })
      .addCase(loadDashboard.rejected, (state, action) => {
        state.loading = false;
        state.error = action.error.message;
      })
      .addCase(loadFeeds.fulfilled, (state, { payload }) => {
        state.feeds = payload;
      })
      .addCase(loadMemories.fulfilled, (state, { payload }) => {
        state.memories = payload;
      })
      .addCase(loadPlans.fulfilled, (state, { payload }) => {
        state.plans = payload;
      })
      .addCase(loadMemos.fulfilled, (state, { payload }) => {
        state.memos = payload.memos;
        state.feishuMemo = payload.status;
      })
      .addCase(loadFeishuStatuses.fulfilled, (state, { payload }) => {
        state.feishu = payload.feishu;
        state.feishuMemo = payload.feishuMemo;
        state.feishuSecret = payload.feishuSecret;
      })
      .addCase(loadVault.fulfilled, (state, { payload }) => {
        state.vault = payload;
      });
  },
});

export const { settingsChanged, vaultChanged, clearError } = appSlice.actions;
export default appSlice.reducer;
