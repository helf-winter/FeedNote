import { ask, message } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { isTauri } from "./api";

let activeCheck: Promise<void> | undefined;

async function runUpdateCheck(): Promise<void> {
  if (!isTauri) return;

  let update;
  try {
    update = await check({ timeout: 15_000 });
  } catch (reason) {
    console.warn("FeedNote update check failed", reason);
    return;
  }

  if (!update) return;

  const accepted = await ask(
    `发现 FeedNote ${update.version}，是否立即下载并安装？\n\n更新包会经过数字签名校验。`,
    {
      title: "FeedNote 更新",
      kind: "info",
      okLabel: "更新并重启",
      cancelLabel: "稍后",
    },
  );
  if (!accepted) {
    await update.close();
    return;
  }

  try {
    await update.downloadAndInstall();
    await relaunch();
  } catch (reason) {
    console.error("FeedNote update installation failed", reason);
    await message(`更新安装失败：${String(reason)}`, {
      title: "FeedNote 更新",
      kind: "error",
      okLabel: "知道了",
    });
  } finally {
    await update.close().catch(() => undefined);
  }
}

export function checkForOnlineUpdate(): Promise<void> {
  if (!activeCheck) {
    activeCheck = runUpdateCheck().finally(() => {
      activeCheck = undefined;
    });
  }
  return activeCheck;
}
