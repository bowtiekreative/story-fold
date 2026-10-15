let token = "";
export async function api<T>(
  url: string,
  options: RequestInit = {},
): Promise<T> {
  if (!token) {
    const session = (await fetch("/api/session").then((r) => r.json())) as {
      token: string;
    };
    token = session.token;
  }
  const headers = new Headers(options.headers);
  if (options.body && !(options.body instanceof FormData))
    headers.set("Content-Type", "application/json");
  if (options.method && !["GET", "HEAD"].includes(options.method))
    headers.set("X-Storyfold-Token", token);
  const response = await fetch("/api" + url, { ...options, headers });
  const data = await response.json();
  if (!response.ok) throw new Error(data.error ?? "The request failed.");
  return data as T;
}
export const send = <T>(url: string, body?: unknown, method = "POST") =>
  api<T>(url, {
    method,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
