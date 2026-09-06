import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { z } from "zod";

function parseEnvFile(path: string): Record<string, string> {
  try {
    return Object.fromEntries(
      readFileSync(path, "utf8")
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line && !line.startsWith("#"))
        .map((line) => {
          const separator = line.indexOf("=");
          return separator < 0
            ? [line, ""]
            : [
                line.slice(0, separator).trim(),
                line.slice(separator + 1).trim(),
              ];
        }),
    );
  } catch {
    return {};
  }
}

const fileEnv = parseEnvFile(
  process.env.FEEDNOTE_SECRETS_FILE ?? resolve("data/secrets.env"),
);
const value = (name: string) => process.env[name] ?? fileEnv[name];

const schema = z.object({
  appId: z.string().startsWith("cli_"),
  appSecret: z.string().min(8),
  baseToken: z.string().min(10),
  tableId: z.string().startsWith("tbl"),
  allowedOpenIds: z.array(z.string().startsWith("ou_")).min(1),
  redirectUri: z.url(),
  webOrigin: z.url(),
  port: z.number().int().min(1).max(65535),
  production: z.boolean(),
});

export type WebConfig = z.infer<typeof schema>;

export function loadConfig(): WebConfig {
  return schema.parse({
    appId: value("FEISHU_APP_ID"),
    appSecret: value("FEISHU_APP_SECRET"),
    baseToken: value("FEISHU_BASE_APP_TOKEN"),
    tableId: value("FEISHU_BASE_TABLE_ID"),
    allowedOpenIds: (value("FEISHU_WEB_ALLOWED_OPEN_IDS") ?? "")
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean),
    redirectUri:
      value("FEISHU_WEB_REDIRECT_URI") ??
      "http://127.0.0.1:4174/api/auth/callback",
    webOrigin: value("FEISHU_WEB_ORIGIN") ?? "http://127.0.0.1:4173",
    port: Number(value("FEISHU_WEB_PORT") ?? 4174),
    production: process.env.NODE_ENV === "production",
  });
}
