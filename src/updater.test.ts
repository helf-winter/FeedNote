import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  ask: vi.fn(),
  check: vi.fn(),
  close: vi.fn(),
  downloadAndInstall: vi.fn(),
  message: vi.fn(),
  relaunch: vi.fn(),
}));

vi.mock("./api", () => ({ isTauri: true }));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  ask: mocks.ask,
  message: mocks.message,
}));
vi.mock("@tauri-apps/plugin-process", () => ({ relaunch: mocks.relaunch }));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));

import { checkForOnlineUpdate } from "./updater";

function availableUpdate() {
  return {
    version: "0.3.0",
    close: mocks.close,
    downloadAndInstall: mocks.downloadAndInstall,
  };
}

describe("online updater", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.close.mockResolvedValue(undefined);
    mocks.downloadAndInstall.mockResolvedValue(undefined);
    mocks.relaunch.mockResolvedValue(undefined);
  });

  it("stays silent when the current version is latest", async () => {
    mocks.check.mockResolvedValue(null);

    await checkForOnlineUpdate();

    expect(mocks.ask).not.toHaveBeenCalled();
  });

  it("keeps the current version when the user postpones", async () => {
    mocks.check.mockResolvedValue(availableUpdate());
    mocks.ask.mockResolvedValue(false);

    await checkForOnlineUpdate();

    expect(mocks.downloadAndInstall).not.toHaveBeenCalled();
    expect(mocks.close).toHaveBeenCalledOnce();
  });

  it("installs a signed accepted update and relaunches", async () => {
    mocks.check.mockResolvedValue(availableUpdate());
    mocks.ask.mockResolvedValue(true);

    await checkForOnlineUpdate();

    expect(mocks.downloadAndInstall).toHaveBeenCalledOnce();
    expect(mocks.relaunch).toHaveBeenCalledOnce();
  });
});
