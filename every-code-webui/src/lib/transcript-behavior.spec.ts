import { describe, expect, it, vi } from 'vitest';

import { TranscriptBehaviorController } from '$lib/transcript-behavior';
import type { ThinkingCell, ToolCallCell } from '$lib/transcript-model';

function completedThinkingCell(id = 'thinking-1'): ThinkingCell {
	return {
		id,
		kind: 'thinking',
		state: 'completed',
		pinned: false,
		timestamp: '12:00 PM',
		duration: '8s',
		summary: 'Summary',
		content: ['Paragraph']
	};
}

function activeThinkingCell(id = 'thinking-1'): ThinkingCell {
	return { ...completedThinkingCell(id), state: 'active', duration: 'live' };
}

function completedToolCell(id = 'tool-1'): ToolCallCell {
	return {
		id,
		kind: 'tool_call',
		state: 'completed',
		pinned: false,
		timestamp: '12:00 PM',
		label: 'Shell command',
		command: 'rg -n test',
		duration: '0.2s',
		exitSummary: 'exit 0',
		preview: 'Preview',
		output: ['line']
	};
}

function runningToolCell(id = 'tool-1'): ToolCallCell {
	return { ...completedToolCell(id), state: 'running', exitSummary: 'running', duration: 'live' };
}

describe('TranscriptBehaviorController', () => {
	it('keeps historical completed thinking compact by default', () => {
		const controller = new TranscriptBehaviorController();
		const cell = completedThinkingCell();

		controller.sync([cell]);

		expect(controller.isThinkingExpanded(cell, false)).toBe(false);
	});

	it('briefly keeps freshly completed thinking expanded before collapsing', () => {
		vi.useFakeTimers();
		const controller = new TranscriptBehaviorController();
		const active = activeThinkingCell();
		const completed = completedThinkingCell(active.id);

		controller.sync([active]);
		expect(controller.isThinkingExpanded(active, false)).toBe(true);

		controller.sync([completed]);
		expect(controller.isThinkingExpanded(completed, false)).toBe(true);
		expect(controller.isThinkingAutoExpanded(completed.id)).toBe(true);

		vi.advanceTimersByTime(5_999);
		expect(controller.isThinkingExpanded(completed, false)).toBe(true);

		vi.advanceTimersByTime(1);
		expect(controller.isThinkingExpanded(completed, false)).toBe(false);
		expect(controller.isThinkingAutoExpanded(completed.id)).toBe(false);

		controller.destroy();
		vi.useRealTimers();
	});

	it('respects manual thinking overrides over collapse timers', () => {
		vi.useFakeTimers();
		const controller = new TranscriptBehaviorController();
		const active = activeThinkingCell();
		const completed = completedThinkingCell(active.id);

		controller.sync([active]);
		controller.sync([completed]);
		controller.toggleThinking(completed, false);

		expect(controller.isThinkingExpanded(completed, false)).toBe(false);

		vi.advanceTimersByTime(6_000);
		expect(controller.isThinkingExpanded(completed, false)).toBe(false);

		controller.toggleThinking(completed, false);
		expect(controller.isThinkingExpanded(completed, false)).toBe(true);

		vi.advanceTimersByTime(6_000);
		expect(controller.isThinkingExpanded(completed, false)).toBe(true);

		controller.destroy();
		vi.useRealTimers();
	});

	it('briefly keeps freshly completed tools expanded before compacting', () => {
		vi.useFakeTimers();
		const controller = new TranscriptBehaviorController();
		const running = runningToolCell();
		const completed = completedToolCell(running.id);

		controller.sync([running]);
		expect(controller.isToolExpanded(running, true)).toBe(true);

		controller.sync([completed]);
		expect(controller.isToolExpanded(completed, true)).toBe(true);
		expect(controller.isToolAutoExpanded(completed.id)).toBe(true);

		vi.advanceTimersByTime(4_000);
		expect(controller.isToolExpanded(completed, true)).toBe(false);
		expect(controller.isToolAutoExpanded(completed.id)).toBe(false);

		controller.destroy();
		vi.useRealTimers();
	});

	it('keeps completed tools expanded when auto compact is disabled globally', () => {
		const controller = new TranscriptBehaviorController();
		const completed = completedToolCell();

		controller.sync([completed]);

		expect(controller.isToolExpanded(completed, false)).toBe(true);
	});
});
