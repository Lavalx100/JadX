export type ConversationProjection = {
  id: string;
  title?: string | null;
  updated_at: string;
  pinned: boolean;
  archived: boolean;
  workspace_id?: string | null;
};

function projectionKey(conversation: ConversationProjection) {
  const title = (conversation.title ?? "New chat").trim().replace(/\s+/g, " ").toLocaleLowerCase();
  // Keep archived and active projections separate so archiving a duplicate
  // never makes it impossible to find from the archived view.
  return `${conversation.archived ? "archived" : "active"}:${conversation.workspace_id ?? "no-workspace"}:${title || "new chat"}`;
}

function isPreferred(left: ConversationProjection, right: ConversationProjection, activeId?: string) {
  if (left.id === activeId) return true;
  if (right.id === activeId) return false;
  if (left.pinned !== right.pinned) return left.pinned;
  return left.updated_at.localeCompare(right.updated_at) > 0;
}

/**
 * Build the compact sidebar projection without deleting anything from the
 * durable transcript. Repeated test runs often leave several conversations
 * with the same generated title; the newest (or pinned) one is the useful
 * entry to show in the navigation.
 */
export function dedupeConversationProjection<T extends ConversationProjection>(items: T[], activeId?: string): T[] {
  const selected = new Map<string, T>();
  for (const item of items) {
    const key = projectionKey(item);
    const current = selected.get(key);
    if (!current || isPreferred(item, current, activeId)) selected.set(key, item);
  }

  return [...selected.values()].sort((left, right) => {
    if (left.archived !== right.archived) return Number(left.archived) - Number(right.archived);
    if (left.pinned !== right.pinned) return Number(right.pinned) - Number(left.pinned);
    return right.updated_at.localeCompare(left.updated_at);
  });
}
