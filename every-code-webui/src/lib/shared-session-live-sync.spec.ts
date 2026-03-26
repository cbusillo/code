import { describe, expect, it } from 'vitest';

import { LIVE_EVENT_REFRESH_DELAY_MS, shouldHydrateThreadFromNotification } from '$lib/shared-session-live-sync';

describe('shared-session-live-sync', () => {
	it('uses a short debounce suitable for live transcript refreshes', () => {
		expect(LIVE_EVENT_REFRESH_DELAY_MS).toBe(250);
	});

	it('refreshes when a raw live event targets the selected thread', () => {
		expect(
			shouldHydrateThreadFromNotification(
				'codex/event/item_updated',
				'thread_selected',
				'thread_selected',
				null
			)
		).toBe(true);
	});

	it('refreshes when a raw live event targets the active thread', () => {
		expect(
			shouldHydrateThreadFromNotification(
				'codex/event/item_completed',
				'thread_live',
				'thread_other',
				'thread_live'
			)
		).toBe(true);
	});

	it('ignores unrelated or typed notifications', () => {
		expect(
			shouldHydrateThreadFromNotification('turn/completed', 'thread_selected', 'thread_selected', null)
		).toBe(false);
		expect(
			shouldHydrateThreadFromNotification(
				'codex/event/item_updated',
				'thread_other',
				'thread_selected',
				null
			)
		).toBe(false);
	});
});
