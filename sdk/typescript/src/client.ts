import type {
  Comment,
  Conversation,
  FeatureState,
  FollowEdge,
  Group,
  GroupOperation,
  GroupRole,
  Id,
  MediaAsset,
  Message,
  ModerationReport,
  ModerationTargetType,
  PinnedMessage,
  Post,
  Profile,
  SavedPost,
  UserSafetyRelationship,
  Visibility,
} from "./types";

export type SocialClientOptions = {
  baseUrl: string;
  appId: Id;
  getUserId: () => Id | Promise<Id>;
  fetch?: typeof globalThis.fetch;
};

export function createSocialClient(options: SocialClientOptions) {
  const request = async <T>(path: string, init: RequestInit = {}): Promise<T> => {
    const userId = await options.getUserId();
    const headers = new Headers(init.headers);
    headers.set("content-type", "application/json");
    headers.set("x-app-id", options.appId);
    headers.set("x-user-id", userId);

    const response = await (options.fetch ?? globalThis.fetch)(`${options.baseUrl.replace(/\/$/, "")}${path}`, {
      ...init,
      headers,
    });
    if (!response.ok) {
      const body = await response.text();
      throw new Error(`social-service ${response.status}: ${body}`);
    }
    if (response.status === 204) {
      return undefined as T;
    }
    return (await response.json()) as T;
  };

  const createGroup = (input: { name: string; avatarMediaId?: Id | null; memberIds?: Id[] }) =>
    request<Group>("/v1/groups", { method: "POST", body: JSON.stringify(input) });
  const group = (groupId: Id) => request<Group>(`/v1/groups/${groupId}`);
  const updateGroup = (groupId: Id, input: { name: string; avatarMediaId?: Id | null }) =>
    request<Group>(`/v1/groups/${groupId}`, { method: "PUT", body: JSON.stringify(input) });
  const addGroupMember = (groupId: Id, userId: Id) =>
    request<Group>(`/v1/groups/${groupId}/members/${userId}`, { method: "PUT" });
  const removeGroupMember = (groupId: Id, userId: Id) =>
    request<Group>(`/v1/groups/${groupId}/members/${userId}`, { method: "DELETE" });
  const setGroupMemberRole = (groupId: Id, userId: Id, role: GroupRole) =>
    request<Group>(`/v1/groups/${groupId}/members/${userId}/role`, {
      method: "PUT",
      body: JSON.stringify({ role }),
    });
  const leaveGroup = (groupId: Id) =>
    request<void>(`/v1/groups/${groupId}/leave`, { method: "POST" });
  const ensureGroupChat = (groupId: Id) =>
    request<Conversation>(`/v1/groups/${groupId}/chat`, { method: "POST" });

  const executeGroupOperation = async (
    operation: GroupOperation,
  ): Promise<Group | Conversation | void> => {
    switch (operation.type) {
      case "create":
        return createGroup(operation);
      case "update":
        return updateGroup(operation.groupId, operation);
      case "addMember":
        return addGroupMember(operation.groupId, operation.userId);
      case "removeMember":
        return removeGroupMember(operation.groupId, operation.userId);
      case "setRole":
        return setGroupMemberRole(operation.groupId, operation.userId, operation.role);
      case "leave":
        return leaveGroup(operation.groupId);
      case "ensureChat":
        return ensureGroupChat(operation.groupId);
    }
  };

  return {
    features: () => request<FeatureState>("/v1/features"),
    report: (input: {
      targetType: ModerationTargetType;
      targetId: Id;
      category: string;
      context?: string | null;
      idempotencyKey?: string | null;
    }) => request<ModerationReport>("/v1/reports", { method: "POST", body: JSON.stringify(input) }),
    profile: (userId: Id) => request<Profile>(`/v1/profiles/${userId}`),
    upsertProfile: (input: {
      displayName: string;
      bio?: string | null;
      avatarMediaId?: Id | null;
      visibility?: Visibility;
    }) => request<Profile>("/v1/profiles/me", { method: "PUT", body: JSON.stringify(input) }),
    registerMedia: (input: { url: string; contentType: string }) =>
      request<MediaAsset>("/v1/media", { method: "POST", body: JSON.stringify(input) }),
    createPost: (input: { body: string; mediaIds?: Id[]; visibility?: Visibility }) =>
      request<Post>("/v1/posts", { method: "POST", body: JSON.stringify(input) }),
    post: (postId: Id) => request<Post>(`/v1/posts/${postId}`),
    deletePost: (postId: Id) => request<void>(`/v1/posts/${postId}`, { method: "DELETE" }),
    savePost: (postId: Id) => request<void>(`/v1/posts/${postId}/save`, { method: "PUT" }),
    unsavePost: (postId: Id) => request<void>(`/v1/posts/${postId}/save`, { method: "DELETE" }),
    savedPosts: (limit = 50) => request<SavedPost[]>(`/v1/saved-posts?limit=${limit}`),
    comments: (postId: Id, limit = 50) => request<Comment[]>(`/v1/posts/${postId}/comments?limit=${limit}`),
    createComment: (postId: Id, body: string) =>
      request<Comment>(`/v1/posts/${postId}/comments`, { method: "POST", body: JSON.stringify({ body }) }),
    follow: (userId: Id) => request<void>(`/v1/follows/${userId}`, { method: "PUT" }),
    unfollow: (userId: Id) => request<void>(`/v1/follows/${userId}`, { method: "DELETE" }),
    followers: (userId: Id, limit = 50) =>
      request<FollowEdge[]>(`/v1/follows/${userId}/followers?limit=${limit}`),
    following: (userId: Id, limit = 50) =>
      request<FollowEdge[]>(`/v1/follows/${userId}/following?limit=${limit}`),
    block: (userId: Id) => request<void>(`/v1/blocks/${userId}`, { method: "PUT" }),
    unblock: (userId: Id) => request<void>(`/v1/blocks/${userId}`, { method: "DELETE" }),
    blocks: (limit = 50) => request<UserSafetyRelationship[]>(`/v1/blocks?limit=${limit}`),
    mute: (userId: Id) => request<void>(`/v1/mutes/${userId}`, { method: "PUT" }),
    unmute: (userId: Id) => request<void>(`/v1/mutes/${userId}`, { method: "DELETE" }),
    mutes: (limit = 50) => request<UserSafetyRelationship[]>(`/v1/mutes?limit=${limit}`),
    timeline: (limit = 50) => request<Post[]>(`/v1/timeline?limit=${limit}`),
    createGroup,
    groups: (limit = 50) => request<Group[]>(`/v1/groups?limit=${limit}`),
    group,
    updateGroup,
    addGroupMember,
    removeGroupMember,
    setGroupMemberRole,
    leaveGroup,
    ensureGroupChat,
    executeGroupOperation,
    createConversation: (memberIds: Id[]) =>
      request<Conversation>("/v1/conversations", { method: "POST", body: JSON.stringify({ memberIds }) }),
    conversations: (limit = 50) => request<Conversation[]>(`/v1/conversations?limit=${limit}`),
    messages: (conversationId: Id, limit = 50) =>
      request<Message[]>(`/v1/conversations/${conversationId}/messages?limit=${limit}`),
    sendMessage: (conversationId: Id, input: { body?: string | null; mediaIds?: Id[] }) =>
      request<Message>(`/v1/conversations/${conversationId}/messages`, {
        method: "POST",
        body: JSON.stringify(input),
      }),
    pinnedMessages: (conversationId: Id, limit = 50) =>
      request<PinnedMessage[]>(`/v1/conversations/${conversationId}/pins?limit=${limit}`),
    pinMessage: (conversationId: Id, messageId: Id) =>
      request<void>(`/v1/conversations/${conversationId}/pins/${messageId}`, { method: "PUT" }),
    unpinMessage: (conversationId: Id, messageId: Id) =>
      request<void>(`/v1/conversations/${conversationId}/pins/${messageId}`, { method: "DELETE" }),
  };
}

export type SocialClient = ReturnType<typeof createSocialClient>;
