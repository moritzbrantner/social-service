# Groups and command boundary

`groups` is a first-class social capability. It owns durable membership and group-local authority; it is not an alias for a chat conversation.

## Domain ownership

- `groups` owns group identity, name, avatar reference, membership, and the `owner | admin | member` role.
- `chat` owns conversations and messages.
- `group_conversations` is an association between those domains. The first implementation supports one primary conversation per group.
- Adding, removing, or leaving a group synchronizes the linked conversation when one exists. Removing membership never rewrites or deletes messages that were already authored.
- Creating/resolving the primary chat is idempotent and also repairs conversation membership from current group membership.
- A group can exist without `chat` enabled. `groups` requires profiles but only integrates with chat and media.

## Minimal authority rules

- The creator becomes the single owner.
- Owners and admins can update group metadata and add members.
- Owners can promote/demote members and transfer ownership.
- Admins can remove ordinary members but cannot remove the owner or another admin.
- The owner must transfer ownership before leaving.
- Only members can read a group. Non-members receive not-found rather than an existence oracle.
- The MVP caps groups at 100 members, matching the existing simple conversation boundary.

Group-local roles are not platform moderation roles. A group owner/admin can manage that group's membership; this does not grant access to moderation queues, account restrictions, or audit APIs.

When moderation is enabled, suspended or banned accounts cannot mutate groups and unavailable accounts cannot be added. Finer-grained group restrictions are intentionally deferred until group policy needs more granularity than account state; group policy does not piggyback on the chat restriction scope.

## Voice and command integration

Speech recognition, natural-language parsing, contact-name resolution, and confirmation UX do not belong in `social-service`.

The TypeScript SDK exposes normal group methods plus a structured `GroupOperation` union. A voice, text, assistant, or automation layer can resolve input such as:

- "Create a group called Family."
- "Add Anna to Family."
- "Make Max an admin."
- "Rename Family to Extended Family."
- "Open the Family chat."
- "Leave Vacation Planning."

into a typed operation and then call `executeGroupOperation`. Rust remains authoritative for app scoping, membership, roles, limits, moderation account state, and persistence.

This preserves one semantic path: spoken commands do not bypass or reimplement group rules.
