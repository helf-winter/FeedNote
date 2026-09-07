import { describe, expect, it } from "vitest";
import {
  FeishuError,
  WEB_OAUTH_SCOPES,
  baseUrlField,
  fromBaseRecord,
  oauthAuthorizeUrl,
  requiresUserReauthorization,
} from "./feishu.js";
import type { WebConfig } from "./env.js";
import { planUpdateSchema } from "../shared/plans.js";

const config: WebConfig = {
  appId: "cli_test",
  appSecret: "not-a-real-secret",
  baseToken: "base-token-value",
  tableId: "tbl123456",
  allowedOpenIds: ["ou_owner"],
  redirectUri: "http://127.0.0.1:4174/api/auth/callback",
  webOrigin: "http://127.0.0.1:4173",
  port: 4174,
  production: false,
};

describe("Feishu Base mapping", () => {
  it("keeps status-only updates sparse", () => {
    expect(planUpdateSchema.parse({ version: 7, status: "done" })).toEqual({
      version: 7,
      status: "done",
    });
  });

  it("maps typed Base cells to the shared plan model", () => {
    const plan = fromBaseRecord({
      record_id: "rec123",
      fields: {
        计划ID: [{ text: "plan-1" }],
        标题: [{ text: "前端面试" }],
        状态: "已安排",
        时间: 1_788_566_400_000,
        链接: { link: "https://example.com/interview", text: "面试入口" },
        标签: ["面试"],
        版本: 3,
        提醒提前分钟: 180,
      },
    });
    expect(plan).toMatchObject({
      recordId: "rec123",
      id: "plan-1",
      title: "前端面试",
      status: "scheduled",
      linkUrl: "https://example.com/interview",
      tags: ["面试"],
      version: 3,
    });
  });

  it("writes URL cells in Feishu's structured shape", () => {
    expect(baseUrlField(" https://example.com/path#section ")).toEqual({
      link: "https://example.com/path#section",
      text: "https://example.com/path#section",
    });
    expect(baseUrlField("  ")).toBeNull();
    expect(baseUrlField(undefined)).toBeNull();
  });

  it("encodes the OAuth callback and state", () => {
    const url = new URL(oauthAuthorizeUrl(config, "state value"));
    expect(url.hostname).toBe("accounts.feishu.cn");
    expect(url.searchParams.get("app_id")).toBe("cli_test");
    expect(url.searchParams.get("state")).toBe("state value");
    expect(url.searchParams.get("redirect_uri")).toBe(config.redirectUri);
    expect(url.searchParams.get("scope")?.split(" ")).toEqual([
      ...WEB_OAUTH_SCOPES,
    ]);
  });

  it("identifies stale user authorization without treating other errors as auth", () => {
    expect(
      requiresUserReauthorization(new FeishuError("reauthorize", 99991679)),
    ).toBe(true);
    expect(requiresUserReauthorization(new FeishuError("forbidden", 403))).toBe(
      false,
    );
  });
});
