import { spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import { resolve } from "node:path";

const env = Object.fromEntries(
  readFileSync(resolve("data/secrets.env"), "utf8")
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter((line) => line && !line.startsWith("#"))
    .map((line) => {
      const index = line.indexOf("=");
      return [line.slice(0, index), line.slice(index + 1)];
    }),
);
const baseToken = env.FEISHU_BASE_APP_TOKEN;
const tableId = env.FEISHU_BASE_TABLE_ID;
const ownerId = (env.FEISHU_WEB_ALLOWED_OPEN_IDS ?? "").split(",")[0]?.trim();
if (!baseToken || !tableId || !ownerId)
  throw new Error("Web/Base configuration is incomplete");

function lark(args) {
  const launcher = resolve(process.env.APPDATA, "npm/lark-cli.ps1");
  const result = spawnSync(
    "powershell.exe",
    ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", launcher, ...args],
    { encoding: "utf8", windowsHide: true },
  );
  if (result.status !== 0) {
    throw new Error(
      result.error?.message ||
        result.stderr ||
        result.stdout ||
        "lark-cli failed",
    );
  }
  const output = JSON.parse(result.stdout);
  if (!output.ok)
    throw new Error(output.error?.message ?? "Feishu request failed");
  return output.data;
}

const existing = lark([
  "base",
  "+record-list",
  "--as",
  "user",
  "--base-token",
  baseToken,
  "--table-id",
  tableId,
  "--limit",
  "1",
  "--format",
  "json",
]);
if ((existing.data ?? []).length > 0) {
  console.log(
    "Base already contains records; migration skipped to avoid duplicates.",
  );
  process.exit(0);
}

const db = new DatabaseSync(resolve("data/feednote.db"), { readOnly: true });
const plans = db
  .prepare(
    `
  SELECT id, title, details, content, link_url, notes, scheduled_at, status,
         source_title, updated_at, reminder_minutes_before, tag
  FROM plans ORDER BY created_at
`,
  )
  .all();

const statusName = (status) =>
  status === "done"
    ? "已完成"
    : status === "needs_clarification"
      ? "待安排"
      : "已安排";
const urlField = (value) => {
  const url = value?.trim();
  return url ? { link: url, text: url } : null;
};
const payloadPath = resolve(".tooling/base-migration-record.json");
mkdirSync(resolve(".tooling"), { recursive: true });
for (const plan of plans) {
  const fields = {
    标题: plan.title,
    状态: statusName(plan.status),
    时间: plan.scheduled_at || null,
    内容: plan.content,
    详情: plan.details,
    链接: urlField(plan.link_url),
    注意事项: plan.notes || null,
    标签: plan.tag ? [plan.tag] : [],
    来源: plan.source_title,
    提醒提前分钟: plan.reminder_minutes_before,
    计划ID: plan.id,
    版本: plan.updated_at,
    更新来源: "desktop",
    所有者ID: ownerId,
    已删除: false,
  };
  writeFileSync(payloadPath, JSON.stringify(fields), "utf8");
  try {
    lark([
      "base",
      "+record-upsert",
      "--as",
      "user",
      "--base-token",
      baseToken,
      "--table-id",
      tableId,
      "--json",
      "@.tooling/base-migration-record.json",
      "--format",
      "json",
    ]);
  } finally {
    try {
      unlinkSync(payloadPath);
    } catch {}
  }
}
console.log(`Migrated ${plans.length} local plans to Feishu Base.`);
