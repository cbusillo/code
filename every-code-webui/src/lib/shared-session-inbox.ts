import type { SharedSessionStructuredRequest } from '$lib/shared-session-domain';
import { groupPendingQuestionRequests } from '$lib/shared-session-question-state';
import type { ProtocolThread } from '$lib/shared-session-types';

export type PendingThreadInboxEntry = {
	threadId: string;
	title: string;
	preview: string;
	cwd: string;
	updatedAt: number;
	pendingActionCount: number;
	pendingApprovalCount: number;
	pendingQuestionCount: number;
	pendingQuestionGroupCount: number;
	isSelected: boolean;
	isLive: boolean;
	summary: string;
};

export function buildPendingThreadInbox(
	threads: ProtocolThread[],
	structuredRequests: SharedSessionStructuredRequest[],
	selectedThreadId: string | null,
	activeThreadId: string | null
): PendingThreadInboxEntry[] {
	const entries = threads
		.map((thread) => {
			const pendingRequests = structuredRequests.filter(
				(request) => request.threadId === thread.id && request.status === 'pending'
			);
			if (!pendingRequests.length) {
				return null;
			}

			const pendingApprovalCount = pendingRequests.filter((request) => request.kind !== 'question').length;
			const pendingQuestions = pendingRequests.filter((request) => request.kind === 'question');
			const pendingQuestionGroupCount = groupPendingQuestionRequests(pendingRequests).length;

			return {
				threadId: thread.id,
				title: thread.preview || thread.id,
				preview: thread.preview,
				cwd: thread.cwd,
				updatedAt: thread.updatedAt,
				pendingActionCount: pendingRequests.length,
				pendingApprovalCount,
				pendingQuestionCount: pendingQuestions.length,
				pendingQuestionGroupCount,
				isSelected: selectedThreadId === thread.id,
				isLive: activeThreadId === thread.id,
				summary: summarizeInboxEntry(pendingApprovalCount, pendingQuestions.length, pendingQuestionGroupCount)
			} satisfies PendingThreadInboxEntry;
		})
		.filter((entry): entry is PendingThreadInboxEntry => Boolean(entry));

	return entries.sort(compareInboxEntries);
}

function compareInboxEntries(a: PendingThreadInboxEntry, b: PendingThreadInboxEntry) {
	if (a.isSelected !== b.isSelected) {
		return a.isSelected ? -1 : 1;
	}

	if (a.isLive !== b.isLive) {
		return a.isLive ? -1 : 1;
	}

	if (a.pendingActionCount !== b.pendingActionCount) {
		return b.pendingActionCount - a.pendingActionCount;
	}

	return b.updatedAt - a.updatedAt;
}

function summarizeInboxEntry(
	pendingApprovalCount: number,
	pendingQuestionCount: number,
	pendingQuestionGroupCount: number
) {
	const parts: string[] = [];

	if (pendingApprovalCount > 0) {
		parts.push(`${pendingApprovalCount} approval${pendingApprovalCount === 1 ? '' : 's'}`);
	}

	if (pendingQuestionGroupCount > 0) {
		parts.push(`${pendingQuestionGroupCount} question set${pendingQuestionGroupCount === 1 ? '' : 's'}`);
	} else if (pendingQuestionCount > 0) {
		parts.push(`${pendingQuestionCount} question${pendingQuestionCount === 1 ? '' : 's'}`);
	}

	if (!parts.length) {
		return 'No pending actions.';
	}

	if (parts.length === 1) {
		return `${parts[0]} waiting.`;
	}

	return `${parts.slice(0, -1).join(', ')} and ${parts.at(-1)} waiting.`;
}
