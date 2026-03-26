import { describe, expect, it } from 'vitest';

import { buildPendingThreadInbox } from '$lib/shared-session-inbox';
import type {
	SharedSessionApprovalRequest,
	SharedSessionQuestionRequest
} from '$lib/shared-session-domain';
import type { ProtocolThread } from '$lib/shared-session-types';

describe('buildPendingThreadInbox', () => {
	it('groups pending work per thread and summarizes question sets', () => {
		const entries = buildPendingThreadInbox(
			[
				makeThread('thread_a', 'Fix reconnect copy', 100),
				makeThread('thread_b', 'Review diff output', 200)
			],
			[
				makeApproval('req_a1', 'thread_a'),
				makeQuestion('req_a2', 'thread_a', 'q_one'),
				makeQuestion('req_a2', 'thread_a', 'q_two'),
				makeQuestion('req_b1', 'thread_b', 'q_alpha')
			],
			null,
			null
		);

		expect(entries).toHaveLength(2);
		expect(entries[0]).toMatchObject({
			threadId: 'thread_a',
			pendingActionCount: 3,
			pendingApprovalCount: 1,
			pendingQuestionCount: 2,
			pendingQuestionGroupCount: 1,
			summary: '1 approval and 1 question set waiting.'
		});
		expect(entries[1]).toMatchObject({
			threadId: 'thread_b',
			summary: '1 question set waiting.'
		});
	});

	it('sorts selected and live blocker threads ahead of older entries', () => {
		const entries = buildPendingThreadInbox(
			[
				makeThread('thread_old', 'Older blocker', 100),
				makeThread('thread_live', 'Live blocker', 150),
				makeThread('thread_selected', 'Selected blocker', 90)
			],
			[
				makeApproval('req_old', 'thread_old'),
				makeApproval('req_live', 'thread_live'),
				makeApproval('req_selected', 'thread_selected')
			],
			'thread_selected',
			'thread_live'
		);

		expect(entries.map((entry) => entry.threadId)).toEqual([
			'thread_selected',
			'thread_live',
			'thread_old'
		]);
	});

	it('ignores answered requests and threads without blockers', () => {
		const entries = buildPendingThreadInbox(
			[makeThread('thread_idle', 'Idle thread', 100), makeThread('thread_busy', 'Busy thread', 200)],
			[
				{ ...makeApproval('req_done', 'thread_idle'), status: 'answered' },
				makeApproval('req_pending', 'thread_busy')
			],
			null,
			null
		);

		expect(entries).toHaveLength(1);
		expect(entries[0]?.threadId).toBe('thread_busy');
	});
});

function makeThread(id: string, preview: string, updatedAt: number): ProtocolThread {
	return {
		id,
		preview,
		modelProvider: 'openai',
		createdAt: updatedAt - 10,
		updatedAt,
		path: `/tmp/${id}.jsonl`,
		cwd: '/Users/cbusillo/Developer/code/every-code-webui',
		cliVersion: '0.0.0',
		source: 'cli',
		gitInfo: null,
		turns: []
	};
}

function makeApproval(requestId: string, threadId: string): SharedSessionApprovalRequest {
	return {
		kind: 'file-approval',
		id: `approval_${requestId}`,
		requestId,
		threadId,
		turnId: `turn_${threadId}`,
		itemId: `item_${threadId}`,
		status: 'pending',
		label: 'Patch approval',
		detail: 'Approve the patch.',
		primary: 'Apply patch',
		secondary: 'Hold for review',
		options: []
	};
}

function makeQuestion(
	requestId: string,
	threadId: string,
	questionId: string
): SharedSessionQuestionRequest {
	return {
		kind: 'question',
		id: `question_${requestId}_${questionId}`,
		requestId,
		threadId,
		turnId: `turn_${threadId}`,
		itemId: `item_${threadId}`,
		questionId,
		status: 'pending',
		header: 'Implementation choices',
		prompt: 'Pick an answer',
		options: [],
		allowOther: true,
		allowSecret: false
	};
}
