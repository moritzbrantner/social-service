export type Id = string;

export type Profile = {
  userId: Id;
  displayName: string;
  bio: string | null;
  avatarMediaId: Id | null;
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

export type Conversation = {
  id: Id;
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

export type Feature = "profiles" | "media" | "posts" | "comments" | "follows" | "chat";
