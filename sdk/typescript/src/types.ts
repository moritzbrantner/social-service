export type Id = string;

export type Visibility = "public" | "private";
export type PostAudience = "public" | "owner_only" | "approved_followers";

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
  audience: PostAudience;
  createdAt: string;
  updatedAt: string;
  version: number;
  mediaIds: Id[];
};

export type SavedPost = Post & {
  savedAt: string;
};

export type Comment = {
  id: Id;
  postId: Id;
  parentCommentId?: Id | null;
  authorId: Id;
  body: string;
  deletedAt?: string | null;
  createdAt: string;
  updatedAt: string;
  version: number;
};

export type ReactionTargetType = "post" | "comment";
export type ReactionType = "like";

export type ReactionCount = {
  reactionType: ReactionType;
  count: number;
};

export type ReactionSummary = {
  counts: ReactionCount[];
  currentUserReactions: ReactionType[];
};

export type VoteTargetType = "post" | "comment";
export type VoteValue = "up" | "down";

export type VoteSummary = {
  upvotes: number;
  downvotes: number;
  score: number;
  currentUserVote: VoteValue | null;
};

export type FollowEdge = {
  followerId: Id;
  followedId: Id;
  createdAt: string;
};

export type FollowRequest = {
  requesterId: Id;
  targetId: Id;
  createdAt: string;
};

export type FollowApproval = {
  requesterId: Id;
  targetId: Id;
  approvedAt: string;
};

export type UserSafetyRelationship = {
  userId: Id;
  createdAt: string;
};

export type GroupRole = "owner" | "admin" | "member";

export type GroupMember = {
  userId: Id;
  role: GroupRole;
  joinedAt: string;
  updatedAt: string;
  version: number;
};

export type Group = {
  id: Id;
  name: string;
  avatarMediaId: Id | null;
  createdBy: Id;
  createdAt: string;
  updatedAt: string;
  version: number;
  members: GroupMember[];
  chatConversationId: Id | null;
};

export type GroupOperation =
  | {
      type: "create";
      name: string;
      avatarMediaId?: Id | null;
      memberIds?: Id[];
    }
  | {
      type: "update";
      groupId: Id;
      name: string;
      avatarMediaId?: Id | null;
    }
  | { type: "addMember"; groupId: Id; userId: Id }
  | { type: "removeMember"; groupId: Id; userId: Id }
  | { type: "setRole"; groupId: Id; userId: Id; role: GroupRole }
  | { type: "leave"; groupId: Id }
  | { type: "ensureChat"; groupId: Id };

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

export type PinnedMessage = Message & {
  pinnedBy: Id;
  pinnedAt: string;
};

export type Feature =
  | "profiles"
  | "media"
  | "posts"
  | "comments"
  | "reactions"
  | "votes"
  | "follows"
  | "follow_requests"
  | "saves"
  | "blocks"
  | "mutes"
  | "groups"
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
  | "group"
  | "conversation"
  | "message";

export type ModerationContentState = "active" | "hidden" | "removed";
export type ModerationAccountState = "active" | "suspended" | "banned";
export type ModerationCaseState = "open" | "investigating" | "resolved" | "dismissed";
export type ModerationRole = "moderator" | "admin";
export type ModerationRestrictionScope =
  | "profile"
  | "media"
  | "post"
  | "comment"
  | "follow"
  | "group"
  | "chat";
export type ModerationCapability =
  | "reports.read"
  | "signals.read"
  | "signals.write"
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

export type ModerationSignalSeverity = "low" | "medium" | "high" | "critical";

export type ModerationSignal = {
  id: Id;
  caseId: Id | null;
  targetType: ModerationTargetType;
  targetId: Id;
  source: string;
  kind: string;
  severity: ModerationSignalSeverity;
  confidence: number | null;
  model: string | null;
  modelVersion: string | null;
  evidence: Record<string, unknown>;
  idempotencyKey: string;
  observedAt: string;
  ingestedBy: Id;
  correlationId: string | null;
  createdAt: string;
};

export type CreateModerationSignalInput = {
  targetType: ModerationTargetType;
  targetId: Id;
  caseId?: Id | null;
  source: string;
  kind: string;
  severity: ModerationSignalSeverity;
  confidence?: number | null;
  model?: string | null;
  modelVersion?: string | null;
  evidence?: Record<string, unknown>;
  idempotencyKey: string;
  observedAt?: string | null;
};

export type ModerationSignalQuery = {
  targetType?: ModerationTargetType;
  targetId?: Id;
  caseId?: Id;
  minimumSeverity?: ModerationSignalSeverity;
  source?: string;
  limit?: number;
};

export type ModerationTargetSnapshot =
  | { type: "profile"; data: Profile }
  | { type: "post"; data: Omit<Post, "mediaIds" | "audience"> }
  | { type: "comment"; data: Comment }
  | { type: "media"; data: MediaAsset }
  | { type: "group"; data: Omit<Group, "members" | "chatConversationId"> }
  | { type: "conversation"; data: Omit<Conversation, "memberIds"> }
  | { type: "message"; data: Omit<Message, "mediaIds"> };
