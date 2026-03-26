import { describe, expect, it } from 'vitest';

import {
	canSubmitQuestionGroup,
	groupedQuestionBatches,
	groupPendingQuestionRequests,
	questionAnswerList,
	questionDraftsByCellId
} from '$lib/shared-session-question-state';
import type { SharedSessionQuestionRequest } from '$lib/shared-session-domain';
import type { ResponderDraftMap } from '$lib/responder-drafts';

describe('shared-session question state', () => {
	it('groups only pending questions by request id', () => {
		const requests: SharedSessionQuestionRequest[] = [
			makeQuestion('q1', 'req-1', 'pending'),
			makeQuestion('q2', 'req-1', 'pending'),
			makeQuestion('q3', 'req-2', 'answered')
		];

		const groups = groupPendingQuestionRequests(requests);

		expect(groups).toHaveLength(1);
		expect(groups[0]?.questions.map((question) => question.id)).toEqual(['q1', 'q2']);
		expect(groupedQuestionBatches(groups)).toHaveLength(1);
	});

	it('maps responder drafts by transcript question cell id', () => {
		const questions = [makeQuestion('q1', 'req-1', 'pending'), makeQuestion('q2', 'req-2', 'pending')];
		const drafts: ResponderDraftMap = {
			'thread-1::turn-1::item-1::question-q1': {
				selectedOption: 'Fast',
				customValue: ''
			}
		};

		expect(questionDraftsByCellId(questions, drafts)).toEqual({
			q1: { selectedOption: 'Fast', customValue: '' },
			q2: { selectedOption: '', customValue: '' }
		});
	});

	it('resolves answers from selected options or custom values', () => {
		const optionQuestion = makeQuestion('q1', 'req-1', 'pending');
		const otherQuestion = makeQuestion('q2', 'req-1', 'pending', { allowOther: true, options: [] });
		const drafts: ResponderDraftMap = {
			'thread-1::turn-1::item-1::question-q1': {
				selectedOption: 'Fast',
				customValue: ''
			},
			'thread-1::turn-1::item-1::question-q2': {
				selectedOption: '__other',
				customValue: 'Ship after QA'
			}
		};

		expect(questionAnswerList(optionQuestion, drafts)).toEqual(['Fast']);
		expect(questionAnswerList(otherQuestion, drafts)).toEqual(['Ship after QA']);
	});

	it('checks grouped batch readiness from shared drafts', () => {
		const questions = [
			makeQuestion('q1', 'req-1', 'pending'),
			makeQuestion('q2', 'req-1', 'pending', { allowOther: true, options: [] })
		];
		const incompleteDrafts: ResponderDraftMap = {
			'thread-1::turn-1::item-1::question-q1': {
				selectedOption: 'Fast',
				customValue: ''
			}
		};
		const completeDrafts: ResponderDraftMap = {
			...incompleteDrafts,
			'thread-1::turn-1::item-1::question-q2': {
				selectedOption: '__other',
				customValue: 'Ship after QA'
			}
		};

		expect(canSubmitQuestionGroup({ questions }, incompleteDrafts)).toBe(false);
		expect(canSubmitQuestionGroup({ questions }, completeDrafts)).toBe(true);
	});
});

function makeQuestion(
	id: string,
	requestId: string,
	status: SharedSessionQuestionRequest['status'],
	overrides: Partial<SharedSessionQuestionRequest> = {}
): SharedSessionQuestionRequest {
	return {
		kind: 'question',
		id,
		requestId,
		threadId: 'thread-1',
		turnId: 'turn-1',
		itemId: 'item-1',
		questionId: `question-${id}`,
		status,
		header: 'Shipping',
		prompt: `Prompt ${id}`,
		options: [{ label: 'Fast', description: 'Fastest option' }],
		allowOther: false,
		allowSecret: false,
		...overrides
	};
}
