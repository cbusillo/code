import type { SharedSessionStructuredRequest } from '$lib/shared-session-domain';
import type { ProtocolThread } from '$lib/shared-session-types';

const STORAGE_KEY = 'every-code-webui.thread-navigation';

export type ThreadNavigationState = {
	pinnedThreadIds: string[];
	openedAtByThreadId: Record<string, number>;
	seenUpdatedAtByThreadId: Record<string, number>;
};

export type ThreadNavigationEntry = {
	thread: ProtocolThread;
	threadId: string;
	pendingCount: number;
	isPinned: boolean;
	isUnread: boolean;
	isSelected: boolean;
	isLive: boolean;
	isLoadedElsewhere: boolean;
	openedAt: number;
	seenUpdatedAt: number;
	recencyAt: number;
};

export const emptyThreadNavigationState = (): ThreadNavigationState => ({
	pinnedThreadIds: [],
	openedAtByThreadId: {},
	seenUpdatedAtByThreadId: {}
});

export function loadThreadNavigationState(storage: Storage): ThreadNavigationState {
	const raw = storage.getItem(STORAGE_KEY);
	if (!raw) {
		return emptyThreadNavigationState();
	}

	try {
		return normalizeThreadNavigationState(JSON.parse(raw));
	} catch {
		return emptyThreadNavigationState();
	}
}

export function persistThreadNavigationState(
	storage: Storage,
	state: ThreadNavigationState
) {
	storage.setItem(STORAGE_KEY, JSON.stringify(normalizeThreadNavigationState(state)));
}

export function pruneThreadNavigationState(
	state: ThreadNavigationState,
	threads: ProtocolThread[]
): ThreadNavigationState {
	const threadIds = new Set(threads.map((thread) => thread.id));
	const pinnedThreadIds = state.pinnedThreadIds.filter((threadId) => threadIds.has(threadId));
	const openedAtByThreadId = Object.fromEntries(
		Object.entries(state.openedAtByThreadId).filter(([threadId]) => threadIds.has(threadId))
	);
	const seenUpdatedAtByThreadId = Object.fromEntries(
		Object.entries(state.seenUpdatedAtByThreadId).filter(([threadId]) => threadIds.has(threadId))
	);

	return {
		pinnedThreadIds,
		openedAtByThreadId,
		seenUpdatedAtByThreadId
	};
}

export function markThreadOpened(
	state: ThreadNavigationState,
	threadId: string,
	at = Date.now()
): ThreadNavigationState {
	if (!threadId || state.openedAtByThreadId[threadId] === at) {
		return state;
	}

	return {
		...state,
		openedAtByThreadId: {
			...state.openedAtByThreadId,
			[threadId]: at
		}
	};
}

export function markThreadSeen(
	state: ThreadNavigationState,
	thread: Pick<ProtocolThread, 'id' | 'updatedAt'> | null | undefined
): ThreadNavigationState {
	if (!thread || !thread.id || state.seenUpdatedAtByThreadId[thread.id] === thread.updatedAt) {
		return state;
	}

	return {
		...state,
		seenUpdatedAtByThreadId: {
			...state.seenUpdatedAtByThreadId,
			[thread.id]: thread.updatedAt
		}
	};
}

export function togglePinnedThread(
	state: ThreadNavigationState,
	threadId: string
): ThreadNavigationState {
	if (!threadId) {
		return state;
	}

	const isPinned = state.pinnedThreadIds.includes(threadId);
	return {
		...state,
		pinnedThreadIds: isPinned
			? state.pinnedThreadIds.filter((entry) => entry !== threadId)
			: [threadId, ...state.pinnedThreadIds]
	};
}

export function buildThreadNavigationEntries(
	threads: ProtocolThread[],
	structuredRequests: SharedSessionStructuredRequest[],
	state: ThreadNavigationState,
	selectedThreadId: string | null,
	activeThreadId: string | null,
	loadedThreadIds: string[],
	attachedThreadIds: string[]
): ThreadNavigationEntry[] {
	const pendingCountByThreadId = countPendingByThreadId(structuredRequests);
	const normalizedState = pruneThreadNavigationState(normalizeThreadNavigationState(state), threads);
	const loadedThreadIdSet = new Set(loadedThreadIds);
	const attachedThreadIdSet = new Set(attachedThreadIds);

	return [...threads]
		.map((thread) => {
			const openedAt = normalizedState.openedAtByThreadId[thread.id] ?? 0;
			const seenUpdatedAt = normalizedState.seenUpdatedAtByThreadId[thread.id] ?? 0;

			return {
				thread,
				threadId: thread.id,
				pendingCount: pendingCountByThreadId.get(thread.id) ?? 0,
				isPinned: normalizedState.pinnedThreadIds.includes(thread.id),
				isUnread: thread.updatedAt > seenUpdatedAt,
				isSelected: thread.id === selectedThreadId,
				isLive: thread.id === activeThreadId,
				isLoadedElsewhere:
					loadedThreadIdSet.has(thread.id) && !attachedThreadIdSet.has(thread.id),
				openedAt,
				seenUpdatedAt,
				recencyAt: Math.max(thread.updatedAt, openedAt)
			};
		})
		.sort(compareThreadNavigationEntries);
}

export function recentThreadEntries(
	entries: ThreadNavigationEntry[],
	limit = 3
): ThreadNavigationEntry[] {
	return entries
		.filter((entry) => entry.openedAt > 0)
		.sort((left, right) => right.openedAt - left.openedAt)
		.slice(0, limit);
}

function normalizeThreadNavigationState(value: unknown): ThreadNavigationState {
	const candidate = isObject(value) ? value : {};
	return {
		pinnedThreadIds: Array.isArray(candidate.pinnedThreadIds)
			? candidate.pinnedThreadIds.filter((entry): entry is string => typeof entry === 'string')
			: [],
		openedAtByThreadId: normalizeNumberRecord(candidate.openedAtByThreadId),
		seenUpdatedAtByThreadId: normalizeNumberRecord(candidate.seenUpdatedAtByThreadId)
	};
}

function normalizeNumberRecord(value: unknown): Record<string, number> {
	if (!isObject(value)) {
		return {};
	}

	return Object.fromEntries(
		Object.entries(value).filter((entry): entry is [string, number] => typeof entry[1] === 'number')
	);
}

function countPendingByThreadId(requests: SharedSessionStructuredRequest[]) {
	const counts = new Map<string, number>();

	for (const request of requests) {
		if (request.status !== 'pending') {
			continue;
		}

		counts.set(request.threadId, (counts.get(request.threadId) ?? 0) + 1);
	}

	return counts;
}

function compareThreadNavigationEntries(left: ThreadNavigationEntry, right: ThreadNavigationEntry) {
	return (
		Number(right.isPinned) - Number(left.isPinned) ||
		Number(right.isSelected) - Number(left.isSelected) ||
		Number(right.isLive) - Number(left.isLive) ||
		Number(right.isLoadedElsewhere) - Number(left.isLoadedElsewhere) ||
		Number(right.isUnread) - Number(left.isUnread) ||
		right.openedAt - left.openedAt ||
		right.thread.updatedAt - left.thread.updatedAt ||
		right.pendingCount - left.pendingCount ||
		left.thread.id.localeCompare(right.thread.id)
	);
}

function isObject(value: unknown): value is Record<string, unknown> {
	return typeof value === 'object' && value !== null;
}
