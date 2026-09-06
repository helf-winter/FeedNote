import { randomBytes } from "node:crypto";
import type { Request, Response } from "express";

export interface Session {
  openId: string;
  name: string;
  avatarUrl?: string;
  accessToken: string;
  refreshToken?: string;
  expiresAt: number;
  csrfToken: string;
}

const sessions = new Map<string, Session>();
const states = new Map<string, number>();
const SESSION_COOKIE = "feednote_session";

function randomToken() {
  return randomBytes(32).toString("base64url");
}

export function issueState() {
  const now = Date.now();
  for (const [key, expiresAt] of states)
    if (expiresAt <= now) states.delete(key);
  const state = randomToken();
  states.set(state, now + 10 * 60_000);
  return state;
}

export function consumeState(state: string) {
  const expiresAt = states.get(state);
  states.delete(state);
  return Boolean(expiresAt && expiresAt > Date.now());
}

function cookies(req: Request) {
  return Object.fromEntries(
    (req.headers.cookie ?? "")
      .split(";")
      .map((part) => part.trim())
      .filter(Boolean)
      .map((part) => {
        const split = part.indexOf("=");
        return [
          part.slice(0, split),
          decodeURIComponent(part.slice(split + 1)),
        ];
      }),
  );
}

export function createSession(
  res: Response,
  session: Omit<Session, "csrfToken">,
  secure: boolean,
) {
  const id = randomToken();
  const value = { ...session, csrfToken: randomToken() };
  sessions.set(id, value);
  res.cookie(SESSION_COOKIE, id, {
    httpOnly: true,
    sameSite: "lax",
    secure,
    path: "/",
    maxAge: 30 * 24 * 60 * 60_000,
  });
  return value;
}

export function getSession(req: Request) {
  const id = cookies(req)[SESSION_COOKIE];
  const session = id ? sessions.get(id) : undefined;
  if (id && session && session.expiresAt + 30 * 24 * 60 * 60_000 < Date.now()) {
    sessions.delete(id);
    return undefined;
  }
  return session;
}

export function clearSession(req: Request, res: Response) {
  const id = cookies(req)[SESSION_COOKIE];
  if (id) sessions.delete(id);
  res.clearCookie(SESSION_COOKIE, { path: "/" });
}

export function requireCsrf(req: Request, session: Session) {
  return req.get("x-csrf-token") === session.csrfToken;
}

export function replaceSessionToken(
  req: Request,
  accessToken: string,
  refreshToken: string | undefined,
  expiresAt: number,
) {
  const id = cookies(req)[SESSION_COOKIE];
  const current = id ? sessions.get(id) : undefined;
  if (!current) return;
  sessions.set(id, { ...current, accessToken, refreshToken, expiresAt });
}
