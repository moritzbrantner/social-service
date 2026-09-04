import type {
  Comment,
  Conversation,
  FeatureState,
  FollowEdge,
  Id,
  MediaAsset,
  Message,
  Post,
  Profile,
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

  return {
    features: () => request<FeatureState>("/v1/features"),
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
    comments: (postId: Id, limit = 50) => request<Comment[]>(`/v1/posts/${postId}/comments?limit=${limit}`),
    createComment: (postId: Id, body: string) =>
      request<Comment>(`/v1/posts/${postId}/comments`, { method: "POST", body: JSON.stringify({ body }) }),
    follow: (userId: Id) => request<void>(`/v1/follows/${userId}`, { method: "PUT" }),
    unfollow: (userId: Id) => request<void>(`/v1/follows/${userId}`, { method: "DELETE" }),
    followers: (userId: Id, limit = 50) =>
      request<FollowEdge[]>(`/v1/follows/${userId}/followers?limit=${limit}`),
    following: (userId: Id, limit = 50) =>
      request<FollowEdge[]>(`/v1/follows/${userId}/following?limit=${limit}`),
    timeline: (limit = 50) => request<Post[]>(`/v1/timeline?limit=${limit}`),
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
  };
}

export type SocialClient = ReturnType<typeof createSocialClient>;
