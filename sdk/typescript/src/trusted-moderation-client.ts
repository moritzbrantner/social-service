import type {
  Id,
  ModerationAccountState,
  ModerationAuditEvent,
  ModerationCapability,
  ModerationCase,
  ModerationCaseState,
  ModerationContentState,
  ModerationMe,
  ModerationRestrictionScope,
  ModerationRole,
  ModerationTargetSnapshot,
  ModerationTargetType,
  UserModeration,
} from "./types";

export type TrustedModerationClientOptions = {
  baseUrl: string;
  appId: Id;
  getUserId: () => Id | Promise<Id>;
  getCapabilities: () => ModerationCapability[] | Promise<ModerationCapability[]>;
  fetch?: typeof globalThis.fetch;
};

export function createTrustedModerationClient(options: TrustedModerationClientOptions) {
  const request = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
    const [userId, capabilities] = await Promise.all([options.getUserId(), options.getCapabilities()]);
    const headers = new Headers(init.headers);
    headers.set("content-type", "application/json");
    headers.set("x-app-id", options.appId);
    headers.set("x-user-id", userId);
    headers.set("x-social-moderation-capabilities", capabilities.join(","));

    const response = await (options.fetch ?? globalThis.fetch)(`${options.baseUrl.replace(/\/$/, "")}${path}`, {
      ...init,
      headers,
    });
    if (!response.ok) {
      const body = await response.text();
      throw new Error(`social-service moderation ${response.status}: ${body}`);
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  };

  return {
    me: () => request<ModerationMe>("/v1/moderation/me"),
    cases: (state?: ModerationCaseState, limit = 50) => {
      const params = new URLSearchParams({ limit: String(limit) });
      if (state) params.set("state", state);
      return request<ModerationCase[]>(`/v1/moderation/cases?${params}`);
    },
    setCaseState: (caseId: Id, state: ModerationCaseState, resolutionNote?: string | null) =>
      request<void>(`/v1/moderation/cases/${caseId}`, {
        method: "PUT",
        body: JSON.stringify({ state, resolutionNote }),
      }),
    reviewTarget: (targetType: ModerationTargetType, targetId: Id) =>
      request<ModerationTargetSnapshot>(`/v1/moderation/content/${targetType}/${targetId}`),
    setContentState: (
      targetType: ModerationTargetType,
      targetId: Id,
      input: { state: ModerationContentState; reason?: string | null; caseId?: Id | null },
    ) =>
      request<void>(`/v1/moderation/content/${targetType}/${targetId}`, {
        method: "PUT",
        body: JSON.stringify(input),
      }),
    user: (userId: Id) => request<UserModeration>(`/v1/moderation/users/${userId}`),
    setAccountState: (
      userId: Id,
      input: { state: ModerationAccountState; reason?: string | null; caseId?: Id | null },
    ) =>
      request<void>(`/v1/moderation/users/${userId}`, {
        method: "PUT",
        body: JSON.stringify(input),
      }),
    setRestriction: (
      userId: Id,
      scope: ModerationRestrictionScope,
      input: { reason?: string | null; caseId?: Id | null } = {},
    ) =>
      request<void>(`/v1/moderation/users/${userId}/restrictions/${scope}`, {
        method: "PUT",
        body: JSON.stringify(input),
      }),
    clearRestriction: (userId: Id, scope: ModerationRestrictionScope) =>
      request<void>(`/v1/moderation/users/${userId}/restrictions/${scope}`, { method: "DELETE" }),
    setRole: (userId: Id, role: ModerationRole, reason?: string | null) =>
      request<void>(`/v1/moderation/roles/${userId}`, {
        method: "PUT",
        body: JSON.stringify({ role, reason }),
      }),
    clearRole: (userId: Id) => request<void>(`/v1/moderation/roles/${userId}`, { method: "DELETE" }),
    audit: (limit = 100) => request<ModerationAuditEvent[]>(`/v1/moderation/audit?limit=${limit}`),
  };
}

export type TrustedModerationClient = ReturnType<typeof createTrustedModerationClient>;
