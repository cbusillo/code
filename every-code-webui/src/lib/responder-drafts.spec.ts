import { describe, expect, it } from 'vitest';

import {
	clearResponderDrafts,
	compactResponderDrafts,
	hasResponderDraftValue,
	loadResponderDrafts,
	persistResponderDrafts,
	responderDraftKey,
	type ResponderDraftMap
} from '$lib/responder-drafts';

describe('responder drafts', () => {
	it('keys drafts by stable protocol identity instead of transient request ids', () => {
		expect(
			responderDraftKey({
				threadId: 'thread_1',
				turnId: 'turn_1',
				itemId: 'item_1',
				questionId: 'shipping_mode'
			})
		).toBe('thread_1::turn_1::item_1::shipping_mode');
	});

	it('round-trips persisted drafts and removes empty entries', () => {
		const storage = makeStorage();
		const drafts: ResponderDraftMap = {
			keep_selected: { selectedOption: 'Fast', customValue: '' },
			keep_custom: { selectedOption: '', customValue: 'Secret token' },
			drop_empty: { selectedOption: '', customValue: '' }
		};

		persistResponderDrafts(storage, drafts);

		expect(loadResponderDrafts(storage)).toEqual({
			keep_selected: { selectedOption: 'Fast', customValue: '' },
			keep_custom: { selectedOption: '', customValue: 'Secret token' }
		});
	});

	it('can clear submitted drafts surgically', () => {
		const drafts: ResponderDraftMap = {
			question_one: { selectedOption: 'Fast', customValue: '' },
			question_two: { selectedOption: '', customValue: 'Ship after QA' }
		};

		expect(clearResponderDrafts(drafts, ['question_one'])).toEqual({
			question_two: { selectedOption: '', customValue: 'Ship after QA' }
		});
	});

	it('detects whether a draft actually contains a value', () => {
		expect(hasResponderDraftValue({ selectedOption: '', customValue: '' })).toBe(false);
		expect(hasResponderDraftValue({ selectedOption: 'Fast', customValue: '' })).toBe(true);
		expect(
			compactResponderDrafts({
				empty: { selectedOption: '', customValue: '' },
				full: { selectedOption: '', customValue: 'value' }
			})
		).toEqual({
			full: { selectedOption: '', customValue: 'value' }
		});
	});
});

function makeStorage(): Storage {
	const values = new Map<string, string>();

	return {
		get length() {
			return values.size;
		},
		clear() {
			values.clear();
		},
		getItem(key) {
			return values.get(key) ?? null;
		},
		key(index) {
			return [...values.keys()][index] ?? null;
		},
		removeItem(key) {
			values.delete(key);
		},
		setItem(key, value) {
			values.set(key, value);
		}
	};
}
