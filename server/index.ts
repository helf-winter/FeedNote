import express, {
  type NextFunction,
  type Request,
  type Response,
} from "express";
import { resolve } from "node:path";
import { ZodError, z } from "zod";
import { planInputSchema, planUpdateSchema } from "../shared/plans.js";
import { loadConfig } from "./env.js";
import {
  BasePlans,
  FeishuError,
  exchangeCode,
  getUser,
  oauthAuthorizeUrl,
  refreshAccessToken,
  requiresUserReauthorization,
} from "./feishu.js";
import {
  clearSession,
  consumeState,
  createSession,
  getSession,
  issueState,
  replaceSessionToken,
  requireCsrf,
  type Session,
} from "./session.js";

const config = loadConfig();
const plans = new BasePlans(config);
const app = express();
app.disable("x-powered-by");
app.use(express.json({ limit: "32kb" }));

app.use((req, res, next) => {
  res.setHeader("X-Content-Type-Options", "nosniff");
  res.setHeader("Referrer-Policy", "no-referrer");
  res.setHeader(
    "Permissions-Policy",
    "camera=(), microphone=(), geolocation=()",
  );
  res.setHeader(
    "Content-Security-Policy",
    "default-src 'self'; img-src 'self' https: data:; style-src 'self'; script-src 'self'; connect-src 'self'",
  );
  next();
});

app.get("/api/health", (_req, res) => res.json({ ok: true }));

app.get("/api/auth/login", (_req, res) => {
  res.redirect(oauthAuthorizeUrl(config, issueState()));
});

app.get("/api/auth/callback", async (req, res, next) => {
  try {
    const code = z.string().min(1).parse(req.query.code);
    const state = z.string().min(1).parse(req.query.state);
    if (!consumeState(state))
      return res.status(400).send("登录状态已过期，请返回重试");
    const token = await exchangeCode(config, code);
    const user = await getUser(token.accessToken);
    if (!config.allowedOpenIds.includes(user.openId))
      return res.status(403).send("该飞书账号尚未获得 FeedNote 访问权限");
    createSession(
      res,
      {
        ...user,
        accessToken: token.accessToken,
        refreshToken: token.refreshToken,
        expiresAt: Date.now() + token.expiresIn * 1000,
      },
      config.production,
    );
    res.redirect(config.webOrigin);
  } catch (error) {
    next(error);
  }
});

async function authenticated(req: Request, res: Response, next: NextFunction) {
  let session = getSession(req);
  if (!session)
    return res
      .status(401)
      .json({ error: "UNAUTHENTICATED", message: "请先使用飞书登录" });
  if (!config.allowedOpenIds.includes(session.openId))
    return res
      .status(403)
      .json({ error: "FORBIDDEN", message: "当前账号未获准访问" });
  if (session.expiresAt < Date.now() + 60_000) {
    if (!session.refreshToken)
      return res
        .status(401)
        .json({ error: "SESSION_EXPIRED", message: "登录已过期，请重新登录" });
    try {
      const token = await refreshAccessToken(config, session.refreshToken);
      replaceSessionToken(
        req,
        token.accessToken,
        token.refreshToken ?? session.refreshToken,
        Date.now() + token.expiresIn * 1000,
      );
      session = getSession(req);
    } catch {
      clearSession(req, res);
      return res
        .status(401)
        .json({ error: "SESSION_EXPIRED", message: "登录已过期，请重新登录" });
    }
  }
  res.locals.session = session;
  next();
}

function current(res: Response): Session {
  return res.locals.session as Session;
}

function mutationGuard(req: Request, res: Response, next: NextFunction) {
  if (!requireCsrf(req, current(res)))
    return res
      .status(403)
      .json({ error: "CSRF", message: "页面安全令牌失效，请刷新页面" });
  next();
}

app.get("/api/session", authenticated, (_req, res) => {
  const session = current(res);
  res.json({
    user: {
      openId: session.openId,
      name: session.name,
      avatarUrl: session.avatarUrl,
    },
    csrfToken: session.csrfToken,
  });
});
app.post("/api/logout", authenticated, mutationGuard, (req, res) => {
  clearSession(req, res);
  res.status(204).end();
});
app.get("/api/plans", authenticated, async (_req, res, next) => {
  try {
    const session = current(res);
    res.json({ plans: await plans.list(session.accessToken, session.openId) });
  } catch (error) {
    next(error);
  }
});
app.post("/api/plans", authenticated, mutationGuard, async (req, res, next) => {
  try {
    const session = current(res);
    res.status(201).json({
      plan: await plans.create(
        session.accessToken,
        session.openId,
        planInputSchema.parse(req.body),
      ),
    });
  } catch (error) {
    next(error);
  }
});
app.patch(
  "/api/plans/:recordId",
  authenticated,
  mutationGuard,
  async (req, res, next) => {
    try {
      const session = current(res);
      const recordId = z.string().startsWith("rec").parse(req.params.recordId);
      res.json({
        plan: await plans.update(
          session.accessToken,
          session.openId,
          recordId,
          planUpdateSchema.parse(req.body),
        ),
      });
    } catch (error) {
      next(error);
    }
  },
);
app.delete(
  "/api/plans/:recordId",
  authenticated,
  mutationGuard,
  async (req, res, next) => {
    try {
      const session = current(res);
      const recordId = z.string().startsWith("rec").parse(req.params.recordId);
      const version = z.coerce
        .number()
        .int()
        .nonnegative()
        .parse(req.query.version);
      await plans.remove(
        session.accessToken,
        session.openId,
        recordId,
        version,
      );
      res.status(204).end();
    } catch (error) {
      next(error);
    }
  },
);

if (config.production) {
  const dist = resolve("dist-web");
  app.use(express.static(dist, { index: false, maxAge: "1h" }));
  app.get(/^(?!\/api(?:\/|$)).*/, (_req, res) =>
    res.sendFile(resolve(dist, "index.html")),
  );
}

app.use((error: unknown, req: Request, res: Response, _next: NextFunction) => {
  if (error instanceof ZodError)
    return res.status(400).json({
      error: "VALIDATION",
      message: error.issues[0]?.message ?? "输入无效",
    });
  if (error instanceof FeishuError) {
    if (requiresUserReauthorization(error)) {
      clearSession(req, res);
      return res.status(401).json({
        error: "REAUTHORIZE_REQUIRED",
        message: "飞书权限已更新，请重新登录以完成授权",
      });
    }
    const status = error.code === 409 ? 409 : error.code === 403 ? 403 : 502;
    return res.status(status).json({ error: "FEISHU", message: error.message });
  }
  console.error(error);
  res.status(500).json({ error: "INTERNAL", message: "服务暂时不可用" });
});

app.listen(config.port, "127.0.0.1", () => {
  console.log(`FeedNote web API listening on http://127.0.0.1:${config.port}`);
});
