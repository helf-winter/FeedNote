import type { CloudPlan, PlanInput, PlanUpdate } from "../shared/plans.js";
import type { WebConfig } from "./env.js";

const API = "https://open.feishu.cn/open-apis";
export const WEB_OAUTH_SCOPES = [
  "auth:user.id:read",
  "offline_access",
  "base:record:retrieve",
  "base:record:create",
  "base:record:update",
] as const;

interface FeishuEnvelope<T> {
  code?: number;
  msg?: string;
  data?: T;
}

export class FeishuError extends Error {
  constructor(
    message: string,
    public readonly code?: number,
  ) {
    super(message);
  }
}

export function requiresUserReauthorization(error: FeishuError) {
  return error.code === 99991679;
}

async function feishuRequest<T>(url: string, init: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  const payload = (await response.json()) as FeishuEnvelope<T> & T;
  if (
    !response.ok ||
    (typeof payload.code === "number" && payload.code !== 0)
  ) {
    throw new FeishuError(
      payload.msg || `飞书接口返回 ${response.status}`,
      payload.code,
    );
  }
  return payload.data ?? (payload as T);
}

export function oauthAuthorizeUrl(config: WebConfig, state: string) {
  const query = new URLSearchParams({
    app_id: config.appId,
    redirect_uri: config.redirectUri,
    scope: WEB_OAUTH_SCOPES.join(" "),
    state,
  });
  return `https://accounts.feishu.cn/open-apis/authen/v1/authorize?${query}`;
}

export interface OAuthToken {
  accessToken: string;
  refreshToken?: string;
  expiresIn: number;
}

function parseOAuthToken(payload: Record<string, unknown>): OAuthToken {
  const accessToken = String(payload.access_token ?? "");
  if (!accessToken) throw new FeishuError("飞书登录没有返回用户访问令牌");
  return {
    accessToken,
    refreshToken: payload.refresh_token
      ? String(payload.refresh_token)
      : undefined,
    expiresIn: Number(payload.expires_in ?? 7200),
  };
}

export async function exchangeCode(config: WebConfig, code: string) {
  const payload = await feishuRequest<Record<string, unknown>>(
    `${API}/authen/v2/oauth/token`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        grant_type: "authorization_code",
        client_id: config.appId,
        client_secret: config.appSecret,
        code,
        redirect_uri: config.redirectUri,
      }),
    },
  );
  return parseOAuthToken(payload);
}

export async function refreshAccessToken(
  config: WebConfig,
  refreshToken: string,
) {
  const payload = await feishuRequest<Record<string, unknown>>(
    `${API}/authen/v2/oauth/token`,
    {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        grant_type: "refresh_token",
        client_id: config.appId,
        client_secret: config.appSecret,
        refresh_token: refreshToken,
      }),
    },
  );
  return parseOAuthToken(payload);
}

export interface FeishuUser {
  openId: string;
  name: string;
  avatarUrl?: string;
}

export async function getUser(accessToken: string): Promise<FeishuUser> {
  const user = await feishuRequest<Record<string, unknown>>(
    `${API}/authen/v1/user_info`,
    {
      headers: { authorization: `Bearer ${accessToken}` },
    },
  );
  const openId = String(user.open_id ?? "");
  if (!openId) throw new FeishuError("飞书登录没有返回 open_id");
  return {
    openId,
    name: String(user.name ?? "飞书用户"),
    avatarUrl: user.avatar_url ? String(user.avatar_url) : undefined,
  };
}

type Fields = Record<string, unknown>;
interface BaseRecord {
  record_id: string;
  fields: Fields;
}

function text(value: unknown): string {
  if (typeof value === "string") return value;
  if (Array.isArray(value)) {
    return value
      .map((item) =>
        typeof item === "string"
          ? item
          : item && typeof item === "object" && "text" in item
            ? String((item as { text: unknown }).text ?? "")
            : "",
      )
      .join("");
  }
  if (value && typeof value === "object") {
    const cell = value as { link?: unknown; text?: unknown };
    if (cell.link != null) return String(cell.link);
    if (cell.text != null) return String(cell.text);
  }
  return value == null ? "" : String(value);
}

export function baseUrlField(value: string | null | undefined): unknown {
  const url = value?.trim();
  return url ? { link: url, text: url } : null;
}

function timestamp(value: unknown): number | undefined {
  const number = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(number) || number <= 0) return undefined;
  return number < 100_000_000_000 ? number * 1000 : number;
}

function statusFromBase(value: unknown): CloudPlan["status"] {
  if (text(value) === "已完成") return "done";
  if (text(value) === "待安排") return "needs_clarification";
  return "scheduled";
}

function statusToBase(value: CloudPlan["status"]) {
  return value === "done"
    ? "已完成"
    : value === "needs_clarification"
      ? "待安排"
      : "已安排";
}

function tags(value: unknown): string[] {
  if (Array.isArray(value)) return value.map(text).filter(Boolean);
  const single = text(value);
  return single ? [single] : [];
}

export function fromBaseRecord(record: BaseRecord): CloudPlan {
  const fields = record.fields ?? {};
  return {
    recordId: record.record_id,
    id: text(fields["计划ID"]) || record.record_id,
    title: text(fields["标题"]) || "未命名计划",
    details: text(fields["详情"]),
    content: text(fields["内容"]),
    linkUrl: text(fields["链接"]) || undefined,
    notes: text(fields["注意事项"]) || undefined,
    scheduledAt: timestamp(fields["时间"]),
    status: statusFromBase(fields["状态"]),
    sourceTitle: text(fields["来源"]),
    updatedAt: timestamp(fields["更新时间"]) ?? Date.now(),
    reminderMinutesBefore: Number(fields["提醒提前分钟"] ?? 180),
    tags: tags(fields["标签"]),
    version: Number(fields["版本"] ?? 0),
  };
}

function toFields(
  input: Partial<PlanInput>,
  id: string,
  version: number,
  ownerId: string,
): Fields {
  const fields: Fields = {
    计划ID: id,
    版本: version,
    更新来源: "web",
    所有者ID: ownerId,
    已删除: false,
  };
  if (input.title !== undefined) fields["标题"] = input.title;
  if (input.status !== undefined) fields["状态"] = statusToBase(input.status);
  if (input.scheduledAt !== undefined) fields["时间"] = input.scheduledAt;
  if (input.content !== undefined) fields["内容"] = input.content;
  if (input.details !== undefined) fields["详情"] = input.details;
  if (input.linkUrl !== undefined) fields["链接"] = baseUrlField(input.linkUrl);
  if (input.notes !== undefined) fields["注意事项"] = input.notes;
  if (input.tags !== undefined) fields["标签"] = input.tags;
  if (input.sourceTitle !== undefined) fields["来源"] = input.sourceTitle;
  if (input.reminderMinutesBefore !== undefined)
    fields["提醒提前分钟"] = input.reminderMinutesBefore;
  return fields;
}

export class BasePlans {
  constructor(private readonly config: WebConfig) {}

  private recordsUrl(recordId?: string) {
    const root = `${API}/bitable/v1/apps/${this.config.baseToken}/tables/${this.config.tableId}/records`;
    return recordId ? `${root}/${recordId}` : root;
  }

  async list(accessToken: string, ownerId: string): Promise<CloudPlan[]> {
    const records: BaseRecord[] = [];
    let pageToken = "";
    do {
      const query = new URLSearchParams({ page_size: "100" });
      if (pageToken) query.set("page_token", pageToken);
      const data = await feishuRequest<{
        items?: BaseRecord[];
        has_more?: boolean;
        page_token?: string;
      }>(`${this.recordsUrl()}?${query}`, {
        headers: { authorization: `Bearer ${accessToken}` },
      });
      records.push(...(data.items ?? []));
      pageToken = data.has_more ? (data.page_token ?? "") : "";
    } while (pageToken);
    return records
      .filter((record) => !record.fields["已删除"])
      .filter((record) => text(record.fields["所有者ID"]) === ownerId)
      .map(fromBaseRecord)
      .sort(
        (a, b) =>
          (a.scheduledAt ?? Number.MAX_SAFE_INTEGER) -
          (b.scheduledAt ?? Number.MAX_SAFE_INTEGER),
      );
  }

  private async getRecord(accessToken: string, recordId: string) {
    const data = await feishuRequest<{ record: BaseRecord }>(
      this.recordsUrl(recordId),
      {
        headers: { authorization: `Bearer ${accessToken}` },
      },
    );
    return data.record;
  }

  async get(accessToken: string, recordId: string) {
    return fromBaseRecord(await this.getRecord(accessToken, recordId));
  }

  async create(accessToken: string, ownerId: string, input: PlanInput) {
    const id = crypto.randomUUID();
    const data = await feishuRequest<{ record: BaseRecord }>(
      this.recordsUrl(),
      {
        method: "POST",
        headers: {
          authorization: `Bearer ${accessToken}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({ fields: toFields(input, id, 1, ownerId) }),
      },
    );
    return fromBaseRecord(data.record);
  }

  async update(
    accessToken: string,
    ownerId: string,
    recordId: string,
    input: PlanUpdate,
  ) {
    const record = await this.getRecord(accessToken, recordId);
    if (text(record.fields["所有者ID"]) !== ownerId)
      throw new FeishuError("无权修改该计划", 403);
    const current = fromBaseRecord(record);
    if (current.version !== input.version) {
      throw new FeishuError("计划已在其他端更新，请刷新后重试", 409);
    }
    const { version: _version, ...patch } = input;
    const data = await feishuRequest<{ record: BaseRecord }>(
      this.recordsUrl(recordId),
      {
        method: "PUT",
        headers: {
          authorization: `Bearer ${accessToken}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({
          fields: toFields(patch, current.id, current.version + 1, ownerId),
        }),
      },
    );
    return fromBaseRecord(data.record);
  }

  async remove(
    accessToken: string,
    ownerId: string,
    recordId: string,
    version: number,
  ) {
    const record = await this.getRecord(accessToken, recordId);
    if (text(record.fields["所有者ID"]) !== ownerId)
      throw new FeishuError("无权删除该计划", 403);
    const current = fromBaseRecord(record);
    if (current.version !== version)
      throw new FeishuError("计划已在其他端更新，请刷新后重试", 409);
    await feishuRequest<{ record: BaseRecord }>(this.recordsUrl(recordId), {
      method: "PUT",
      headers: {
        authorization: `Bearer ${accessToken}`,
        "content-type": "application/json",
      },
      body: JSON.stringify({
        fields: {
          已删除: true,
          版本: version + 1,
          更新来源: "web",
          所有者ID: ownerId,
        },
      }),
    });
  }
}
