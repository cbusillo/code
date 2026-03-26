import type { DiffCell, ThinkingCell, ToolCallCell, TranscriptCell } from '$lib/transcript-model';

export type DiffView = 'compact' | 'expanded' | 'full';

const THINKING_COLLAPSE_DELAY_MS = 6_000;
const TOOL_COLLAPSE_DELAY_MS = 4_000;

type Notify = () => void;
type TimerHandle = ReturnType<typeof setTimeout>;

export class TranscriptBehaviorController {
	#notify: Notify;
	#thinkingManual = new Map<string, boolean>();
	#toolManual = new Map<string, boolean>();
	#diffManual = new Map<string, DiffView>();
	#thinkingAutoExpanded = new Set<string>();
	#toolAutoExpanded = new Set<string>();
	#snapshots = new Map<string, string>();
	#timers = new Map<string, TimerHandle>();

	constructor(notify: Notify = () => {}) {
		this.#notify = notify;
	}

	destroy() {
		for (const timer of this.#timers.values()) {
			clearTimeout(timer);
		}

		this.#timers.clear();
	}

	sync(cells: TranscriptCell[]) {
		const nextIds = new Set<string>();

		for (const cell of cells) {
			nextIds.add(cell.id);
			const previous = this.#snapshots.get(cell.id);
			this.#snapshots.set(cell.id, snapshotKey(cell));

			if (cell.kind === 'thinking') {
				this.#syncThinking(cell, previous);
				continue;
			}

			if (cell.kind === 'tool_call') {
				this.#syncTool(cell, previous);
			}
		}

		for (const cellId of [...this.#snapshots.keys()]) {
			if (nextIds.has(cellId)) {
				continue;
			}

			this.#snapshots.delete(cellId);
			this.#thinkingManual.delete(cellId);
			this.#toolManual.delete(cellId);
			this.#diffManual.delete(cellId);
			this.#thinkingAutoExpanded.delete(cellId);
			this.#toolAutoExpanded.delete(cellId);
			this.#clearTimer(timerKey('thinking', cellId));
			this.#clearTimer(timerKey('tool', cellId));
		}
	}

	isThinkingExpanded(cell: ThinkingCell, showAllReasoning: boolean) {
		if (cell.state === 'active' || cell.pinned || showAllReasoning) {
			return true;
		}

		return this.#thinkingManual.get(cell.id) ?? this.#thinkingAutoExpanded.has(cell.id);
	}

	isThinkingAutoExpanded(cellId: string) {
		return this.#thinkingAutoExpanded.has(cellId);
	}

	isToolExpanded(cell: ToolCallCell, autoCompactTools: boolean) {
		if (cell.state === 'running' || cell.pinned || !autoCompactTools) {
			return true;
		}

		return this.#toolManual.get(cell.id) ?? this.#toolAutoExpanded.has(cell.id);
	}

	isToolAutoExpanded(cellId: string) {
		return this.#toolAutoExpanded.has(cellId);
	}

	diffView(cell: DiffCell) {
		return this.#diffManual.get(cell.id) ?? (cell.autoCompact ? 'compact' : 'expanded');
	}

	toggleThinking(cell: ThinkingCell, showAllReasoning: boolean) {
		const expanded = this.isThinkingExpanded(cell, showAllReasoning);
		this.#thinkingAutoExpanded.delete(cell.id);
		this.#clearTimer(timerKey('thinking', cell.id));
		this.#thinkingManual.set(cell.id, !expanded);
		this.#notify();
	}

	toggleTool(cell: ToolCallCell, autoCompactTools: boolean) {
		const expanded = this.isToolExpanded(cell, autoCompactTools);
		this.#toolAutoExpanded.delete(cell.id);
		this.#clearTimer(timerKey('tool', cell.id));
		this.#toolManual.set(cell.id, !expanded);
		this.#notify();
	}

	cycleDiffView(cellId: string) {
		const current = this.#diffManual.get(cellId) ?? 'compact';
		const next = current === 'compact' ? 'expanded' : current === 'expanded' ? 'full' : 'compact';
		this.#diffManual.set(cellId, next);
		this.#notify();
	}

	setDiffView(cellId: string, view: DiffView) {
		this.#diffManual.set(cellId, view);
		this.#notify();
	}

	#syncThinking(cell: ThinkingCell, previous: string | undefined) {
		const key = timerKey('thinking', cell.id);

		if (cell.state === 'active') {
			this.#thinkingAutoExpanded.add(cell.id);
			this.#clearTimer(key);
			return;
		}

		if (cell.pinned || this.#thinkingManual.has(cell.id)) {
			this.#clearTimer(key);
			return;
		}

		if (previous === 'thinking:active') {
			this.#thinkingAutoExpanded.add(cell.id);
			this.#scheduleCollapse(key, THINKING_COLLAPSE_DELAY_MS, () => {
				this.#thinkingAutoExpanded.delete(cell.id);
			});
			return;
		}

		this.#thinkingAutoExpanded.delete(cell.id);
		this.#clearTimer(key);
	}

	#syncTool(cell: ToolCallCell, previous: string | undefined) {
		const key = timerKey('tool', cell.id);

		if (cell.state === 'running') {
			this.#toolAutoExpanded.add(cell.id);
			this.#clearTimer(key);
			return;
		}

		if (cell.pinned || this.#toolManual.has(cell.id)) {
			this.#clearTimer(key);
			return;
		}

		if (previous === 'tool_call:running') {
			this.#toolAutoExpanded.add(cell.id);
			this.#scheduleCollapse(key, TOOL_COLLAPSE_DELAY_MS, () => {
				this.#toolAutoExpanded.delete(cell.id);
			});
			return;
		}

		this.#toolAutoExpanded.delete(cell.id);
		this.#clearTimer(key);
	}

	#scheduleCollapse(key: string, delayMs: number, onExpire: () => void) {
		this.#clearTimer(key);
		const handle = setTimeout(() => {
			this.#timers.delete(key);
			onExpire();
			this.#notify();
		}, delayMs);
		this.#timers.set(key, handle);
		this.#notify();
	}

	#clearTimer(key: string) {
		const handle = this.#timers.get(key);
		if (!handle) {
			return;
		}

		clearTimeout(handle);
		this.#timers.delete(key);
	}
}

function snapshotKey(cell: TranscriptCell) {
	if (cell.kind === 'thinking' || cell.kind === 'tool_call') {
		return `${cell.kind}:${cell.state}`;
	}

	return cell.kind;
}

function timerKey(kind: 'thinking' | 'tool', cellId: string) {
	return `${kind}:${cellId}`;
}
