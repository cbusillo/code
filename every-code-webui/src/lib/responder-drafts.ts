import type { SharedSessionQuestionRequest } from '$lib/shared-session-domain';

const STORAGE_KEY = 'every-code-webui.responder-drafts:v1';

export type ResponderDraft = {
	selectedOption: string;
	customValue: string;
};

export type ResponderDraftMap = Record<string, ResponderDraft>;

export function responderDraftKey(
	question: Pick<SharedSessionQuestionRequest, 'threadId' | 'turnId' | 'itemId' | 'questionId'>
) {
	return `${question.threadId}::${question.turnId}::${question.itemId}::${question.questionId}`;
}

export function loadResponderDrafts(storage: Storage | null | undefined): ResponderDraftMap {
	const rawValue = storage?.getItem(STORAGE_KEY);
	if (!rawValue) {
		return {};
	}

	try {
		const parsed = JSON.parse(rawValue) as Record<string, Partial<ResponderDraft>>;
		return Object.fromEntries(
			Object.entries(parsed)
				.filter(([, value]) => typeof value === 'object' && value !== null)
				.map(([key, value]) => [
					key,
					{
						selectedOption: typeof value.selectedOption === 'string' ? value.selectedOption : '',
						customValue: typeof value.customValue === 'string' ? value.customValue : ''
					}
				])
		);
	} catch {
		storage?.removeItem(STORAGE_KEY);
		return {};
	}
}

export function persistResponderDrafts(
	storage: Storage | null | undefined,
	drafts: ResponderDraftMap
) {
	if (!storage) {
		return;
	}

	const compacted = compactResponderDrafts(drafts);
	if (!Object.keys(compacted).length) {
		storage.removeItem(STORAGE_KEY);
		return;
	}

	storage.setItem(STORAGE_KEY, JSON.stringify(compacted));
}

export function clearResponderDrafts(
	drafts: ResponderDraftMap,
	keysToClear: string[]
): ResponderDraftMap {
	const nextDrafts = { ...drafts };
	for (const key of keysToClear) {
		delete nextDrafts[key];
	}

	return nextDrafts;
}

export function compactResponderDrafts(drafts: ResponderDraftMap): ResponderDraftMap {
	return Object.fromEntries(
		Object.entries(drafts).filter(([, value]) => hasResponderDraftValue(value))
	);
}

export function hasResponderDraftValue(draft: ResponderDraft | undefined) {
	if (!draft) {
		return false;
	}

	return Boolean(draft.selectedOption.trim() || draft.customValue.trim());
}
