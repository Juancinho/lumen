/** Walks every page of a cursor-paginated API. */
export async function fetchAll<T>(get: (cursor?: string) => Promise<{ items: T[]; next?: string }>): Promise<T[]> {
  const out: T[] = [];
  let cursor: string | undefined;
  do {
    const page = await get(cursor);
    out.push(...page.items);
    cursor = page.next;
  } while (cursor);
  return out;
}
