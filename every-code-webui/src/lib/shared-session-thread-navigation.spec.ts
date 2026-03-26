import { describe, expect, it } from 'vitest';

import type { SharedSessionStructuredRequest } from '$lib/shared-session-domain';
import {
	buildThreadNavigationEntries,
	emptyThreadNavigationState,
	markThreadOpened,
	markThreadSeen,
	recentThreadEntries,
	togglePinnedThread,
	type ThreadNavigationState
} from '$lib/shared-session-thread-navigation';
import type { ProtocolThread } from '$lib/shared-session-types';

describe('shared-session-thread-navigation', () => {
	it('sorts pinned, selected, unread, and recent threads in a stable order', () => {
		const threads = [
			makeThread('thread_unread', 20),
			makeThread('thread_pinned', 10),
			makeThread('thread_recent', 15),
			makeThread('thread_idle', 5)
		];
		const requests = [makePendingApproval('thread_unread')] satisfies SharedSessionStructuredRequest[];
		const state: ThreadNavigationState = {
			pinnedThreadIds: ['thread_pinned'],
			openedAtByThreadId: { thread_recent: 3000, thread_idle: 1000 },
			seenUpdatedAtByThreadId: {
				thread_pinned: 10,
				thread_recent: 15,
				thread_idle: 5
			}
		};

		const entries = buildThreadNavigationEntries(
			threads,
			requests,
			state,
			'thread_recent',
			null,
			['thread_idle'],
			[]
		);

		expect(entries.map((entry) => entry.threadId)).toEqual([
			'thread_pinned',
			'thread_recent',
			'thread_idle',
			'thread_unread'
		]);
		expect(entries[2]).toMatchObject({ isLoadedElsewhere: true });
		expect(entries[3]).toMatchObject({ isUnread: true, pendingCount: 1 });
	});

	it('marks opened and seen threads without mutating unrelated fields', () => {
		const opened = markThreadOpened(emptyThreadNavigationState(), 'thread_alpha', 42);
		const seen = markThreadSeen(opened, makeThread('thread_alpha', 99));

		expect(opened.openedAtByThreadId.thread_alpha).toBe(42);
		expect(seen.seenUpdatedAtByThreadId.thread_alpha).toBe(99);
		expect(seen.openedAtByThreadId.thread_alpha).toBe(42);
	});

	it('toggles pin state and derives recently opened threads', () => {
		const pinnedState = togglePinnedThread(emptyThreadNavigationState(), 'thread_beta');
		expect(pinnedState.pinnedThreadIds).toEqual(['thread_beta']);

		const entries = buildThreadNavigationEntries(
			[
				makeThread('thread_alpha', 10),
				makeThread('thread_beta', 20),
				makeThread('thread_gamma', 30)
			],
			[],
			{
				...pinnedState,
				openedAtByThreadId: { thread_alpha: 100, thread_gamma: 200, thread_beta: 50 }
			},
			null,
			null,
			[],
			[]
		);

		expect(recentThreadEntries(entries, 2).map((entry) => entry.threadId)).toEqual([
			'thread_gamma',
			'thread_alpha'
		]);
	});
});

function makeThread(id: string, updatedAt: number): ProtocolThread {
	return {
		id,
		preview: id,
		createdAt: updatedAt - 5,
		cwd: `/tmp/${id}`,
		updatedAt,
		path: null,
		turns: [],
		gitInfo: null,
		modelProvider: 'openai',
		cliVersion: '0.0.0',
		source: 'cli'
	};
}

function makePendingApproval(threadId: string): SharedSessionStructuredRequest {
	return {
		kind: 'file-approval',
		id: `approval_${threadId}`,
		requestId: 1,
		threadId,
		turnId: 'turn_1',
		itemId: 'item_1',
		status: 'pending',
		label: 'Approve',
		detail: 'Approve change',
		primary: 'Approve',
		secondary: 'Decline',
		options: [],
		resolution: undefined,
		respondedBy: undefined
	};
}
