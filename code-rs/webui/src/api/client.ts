type FetchOptions = RequestInit & {
  token?: string;
};

export const apiFetch = async <T>(path: string, options: FetchOptions = {}) => {
  const { token, headers, ...rest } = options;
  const response = await fetch(path, {
    ...rest,
    headers: {
      "Content-Type": "application/json",
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
      ...(headers || {}),
    },
  });

  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    const message = body.error || response.statusText;
    const error = new Error(message);
    (error as Error & { status?: number }).status = response.status;
    throw error;
  }

  return response.json() as Promise<T>;
};
