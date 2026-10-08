/** Retries fetch with exponential backoff. */
export async function fetchWithRetry(url: string, attempts = 4): Promise<Response> {
  for (let i = 0; i < attempts; i++) {
    try {
      return await fetch(url);
    } catch {
      await new Promise((r) => setTimeout(r, 2 ** i * 1000));
    }
  }
  throw new Error("failed");
}
