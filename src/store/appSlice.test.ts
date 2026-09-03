import { describe, expect, it } from "vitest";
import reducer, {
  defaultSettings,
  loadDashboard,
  loadVault,
  settingsChanged,
  vaultChanged,
} from "./appSlice";

describe("appSlice", () => {
  it("updates the typed settings snapshot", () => {
    const state = reducer(
      undefined,
      settingsChanged({ ...defaultSettings, launchAtLogin: true }),
    );
    expect(state.settings.launchAtLogin).toBe(true);
  });

  it("stores loaded domain snapshots", () => {
    const state = reducer(
      undefined,
      loadDashboard.fulfilled(
        {
          feeds: [],
          memories: [],
          reviews: [],
          plans: [],
          settings: defaultSettings,
          stats: {
            totalFeeds: 3,
            totalMemories: 2,
            pendingReviews: 1,
            pendingProcessing: 0,
          },
        },
        "request",
        undefined,
      ),
    );
    expect(state.loading).toBe(false);
    expect(state.stats.totalFeeds).toBe(3);
  });

  it("tracks vault metadata without storing decrypted records", () => {
    const unlocked = reducer(
      undefined,
      loadVault.fulfilled(
        { initialized: true, unlocked: true, secretCount: 1 },
        "request",
        undefined,
      ),
    );
    const locked = reducer(
      unlocked,
      vaultChanged({ initialized: true, unlocked: false, secretCount: 1 }),
    );
    expect(locked.vault.unlocked).toBe(false);
    expect("secrets" in locked).toBe(false);
  });
});
