import type {
	SharedSessionQuestionRequest,
	SharedSessionStructuredRequest
} from '$lib/shared-session-domain';
import type { ResponderDraftMap } from '$lib/responder-drafts';
import { responderDraftKey } from '$lib/responder-drafts';

export type PendingQuestionGroup = {
	requestId: SharedSessionQuestionRequest['requestId'];
	header: string;
	questions: SharedSessionQuestionRequest[];
};

export function groupPendingQuestionRequests(
	requests: SharedSessionStructuredRequest[]
): PendingQuestionGroup[] {
	const groups = new Map<string, PendingQuestionGroup>();

	for (const request of requests) {
		if (request.kind !== 'question' || request.status !== 'pending') {
			continue;
		}

		const key = String(request.requestId);
		const existing = groups.get(key);
		if (existing) {
			existing.questions.push(request);
			continue;
		}

		groups.set(key, {
			requestId: request.requestId,
			header: request.header,
			questions: [request]
		});
	}

	return [...groups.values()];
}

export function groupedQuestionBatches(groups: PendingQuestionGroup[]) {
	return groups.filter((group) => group.questions.length > 1);
}

export function questionDraftsByCellId(
	questions: SharedSessionQuestionRequest[],
	drafts: ResponderDraftMap
) {
	return Object.fromEntries(
		questions.map((question) => [
			question.id,
			drafts[responderDraftKey(question)] ?? {
				selectedOption: '',
				customValue: ''
			}
		])
	);
}

export function questionAnswerList(
	question: SharedSessionQuestionRequest,
	drafts: ResponderDraftMap
) {
	const draft = drafts[responderDraftKey(question)] ?? {
		selectedOption: '',
		customValue: ''
	};

	if (draft.selectedOption && draft.selectedOption !== '__other') {
		return [draft.selectedOption];
	}

	if (
		(question.allowOther || question.allowSecret || !question.options.length) &&
		draft.customValue.trim()
	) {
		return [draft.customValue.trim()];
	}

	return [];
}

export function canSubmitQuestionGroup(
	group: Pick<PendingQuestionGroup, 'questions'>,
	drafts: ResponderDraftMap
) {
	return group.questions.every((question) => questionAnswerList(question, drafts).length > 0);
}
