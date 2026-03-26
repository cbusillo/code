export const LIVE_EVENT_REFRESH_DELAY_MS = 250;

export function shouldHydrateThreadFromNotification(
	method: string,
	threadId: string | null,
	selectedThreadId: string | null,
	activeThreadId: string | null
) {
	if (!threadId || !method.startsWith('codex/event/')) {
		return false;
	}

	return threadId === selectedThreadId || threadId === activeThreadId;
}
