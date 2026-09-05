export type Id = string;

export type Visibility = "public" | "private";

export type Profile = {
  userId: Id;
  displayName: string;
  bio: string | null;
  avatarMediaId: Id | null;
  visibility: Visibility;
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type MediaAsset = {
  id: Id;
  ownerId: Id;
  url: string;
  contentType: string;
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type Post = {
  id: Id;
  authorId: Id;
  body: string;
  visibility: Visibility;
  createdAt: string;
  updatedAt: string;
  version: number;
  mediaIds: Id[];
};

export type Comment = {
  id: Id;
  postId: Id;
  authorId: Id;
  body: string;
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type FollowEdge = {
  followerId: Id;
  followedId: Id;
  createdAt: string;
};

export type Conversation = {
  id: Id;
  memberIds: Id[];
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type Message = {
  id: Id;
  conversationId: Id;
  authorId: Id;
  body: string | null;
  createdAt: string;
  updatedAt: string;
  version: number;
  mediaIds: Id[];
};

export type Feature =
  | "profiles"
  | "media"
  | "posts"
  | "comments"
  | "follows"
  | "chat"
  | "moderation";

export type FeatureState = {
  enabled: Feature[];
  implemented: Feature[];
  deploymentSupported: Feature[];
  appRequested: Feature[];
  effective: Feature[];
};

export type ModerationTargetType =
  | "profile"
  | "post"
  | "comment"
  | "media"
  | "conversation"
  | "message";

export type ModerationContentState = "active" | "hidden" | "removed";
export type ModerationAccountState = "active" | "suspended" | "banned";
export type ModerationCaseState = "open" | "investigating" | "resolved" | "dismissed";
export type ModerationRole = "moderator" | "admin";
export type ModerationRestrictionScope = "profile" | "media" | "post" | "comment" | "follow" | "chat";
export type ModerationCapability =
  | "reports.read"
  | "content.moderate"
  | "users.restrict"
  | "roles.manage"
  | "audit.read";

export type ModerationReport = {
  id: Id;
  caseId: Id;
  reporterId: Id;
  targetType: ModerationTargetType;
  targetId: Id;
  category: string;
  context: string | null;
  idempotencyKey: string | null;
  createdAt: string;
};

export type ModerationCase = {
  id: Id;
  targetType: ModerationTargetType;
  targetId: Id;
  state: ModerationCaseState;
  openedBy: Id;
  resolutionNote: string | null;
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type ModerationMe = {
  userId: Id;
  role: ModerationRole | null;
  effectiveCapabilities: ModerationCapability[];
};

export type ModerationRestriction = {
  scope: ModerationRestrictionScope;
  reason: string | null;
  updatedAt: string;
  version: number;
};

export type UserModeration = {
  userId: Id;
  state: ModerationAccountState;
  restrictions: ModerationRestriction[];
};

export type ModerationAuditEvent = {
  id: Id;
  actorId: Id;
  action: string;
  targetKind: string;
  targetId: Id | null;
  reason: string | null;
  previousState: string | null;
  newState: string | null;
  caseId: Id | null;
  correlationId: string | null;
  createdAt: string;
};

export type ModerationTargetSnapshot =
  | { type: "profile"; data: Profile }
  | { type: "post"; data: Omit<Post, "mediaIds"> }
  | { type: "comment"; data: Comment }
  | { type: "media"; data: MediaAsset }
  | { type: "conversation"; data: Omit<Conversation, "memberIds"> }
  | { type: "message"; data: Omit<Message, "mediaIds"> };
