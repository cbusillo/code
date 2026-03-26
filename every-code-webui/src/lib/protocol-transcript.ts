import type {
	ProtocolThread,
	ProtocolThreadItem,
	ProtocolTurn,
	ProtocolUserInput
} from '$lib/shared-session-types';
import type {
	AssistantMessageCell,
	ApprovalCell,
	DiffCell,
	QuestionCell,
	RichBlock,
	ToolCallCell,
	TranscriptCell,
	UserMessageCell
} from '$lib/transcript-model';
import type { SharedSessionStructuredRequest } from '$lib/shared-session-domain';

const ANSI_ESCAPE_PATTERN = new RegExp(String.raw`\x1B(?:[@-Z\\-_]|\[[0-?]*[ -/]*[@-~])`, 'g');

export function protocolThreadToTranscriptCells(thread: ProtocolThread): TranscriptCell[] {
	return thread.turns.flatMap((turn, turnIndex) => mapTurnToCells(thread, turn, turnIndex));
}

export function structuredRequestsToTranscriptCells(
	requests: SharedSessionStructuredRequest[],
	timestamp = 'live'
): TranscriptCell[] {
	const questionCounts = new Map<string, number>();
	for (const request of requests) {
		if (request.kind !== 'question') {
			continue;
		}

		const key = String(request.requestId);
		questionCounts.set(key, (questionCounts.get(key) ?? 0) + 1);
	}

	return requests.map((request) => {
		if (request.kind === 'question') {
			return {
				id: request.id,
				kind: 'question',
				status: request.status,
				timestamp,
				header: request.header,
				prompt: request.prompt,
				options: request.options,
				allowOther: request.allowOther,
				allowSecret: request.allowSecret,
				questionCountInGroup: questionCounts.get(String(request.requestId)) ?? 1,
				answer: request.answer,
				respondedBy: request.respondedBy
			} satisfies QuestionCell;
		}

		return {
			id: request.id,
			kind: 'approval',
			status: request.status,
			timestamp,
			label: request.label,
			detail: request.detail,
			primary: request.primary,
			secondary: request.secondary,
			resolution: request.resolution,
			respondedBy: request.respondedBy
		} satisfies ApprovalCell;
	});
}

function mapTurnToCells(
	thread: ProtocolThread,
	turn: ProtocolTurn,
	turnIndex: number
): TranscriptCell[] {
	const timestamp = formatTimestamp(
		turn.status === 'inProgress' ? thread.updatedAt : thread.createdAt
	);
	const cells = turn.items.flatMap((item, itemIndex) =>
		mapItemToCells(item, turn, turnIndex, itemIndex, timestamp)
	);

	if (turn.status === 'failed' && turn.error) {
		cells.push({
			id: `${turn.id}_error`,
			kind: 'error',
			status: 'failed',
			timestamp,
			message: turn.error.message,
			detail: turn.error.additionalDetails ?? 'The app-server reported a failed turn.'
		});
	}

	return cells;
}

function mapItemToCells(
	item: ProtocolThreadItem,
	turn: ProtocolTurn,
	turnIndex: number,
	itemIndex: number,
	timestamp: string
): TranscriptCell[] {
	const ordinal = `${turnIndex + 1}.${itemIndex + 1}`;

	switch (item.type) {
		case 'userMessage':
			return [
				{
					id: item.id,
					kind: 'user_message',
					timestamp,
					text: summarizeUserInputs(item.content)
				} satisfies UserMessageCell
			];
		case 'agentMessage':
			return [toAssistantMessageCell(item.id, timestamp, item.text, `Response ${ordinal}`)];
		case 'plan':
			return [toAssistantMessageCell(item.id, timestamp, item.text, `Plan ${ordinal}`)];
		case 'reasoning':
			return [
				{
					id: item.id,
					kind: 'thinking',
					timestamp,
					state: turn.status === 'inProgress' ? 'active' : 'completed',
					pinned: false,
					duration: turn.status === 'inProgress' ? 'live' : 'synced',
					summary: item.summary[0] ?? item.content[0] ?? `Reasoning ${ordinal}`,
					content: item.content.length ? item.content : item.summary
				}
			];
		case 'commandExecution':
			return [toCommandCell(item, timestamp)];
		case 'fileChange':
			return [toDiffCell(item, timestamp)];
		case 'mcpToolCall':
			return [
				{
					id: item.id,
					kind: 'tool_call',
					timestamp,
					state: mapToolStatus(item.status),
					pinned: false,
					label: 'MCP tool call',
					command: `${item.server}/${item.tool}`,
					duration: formatDuration(item.durationMs),
					exitSummary: item.status,
					preview: summarizeMcpCall(item),
					output: compactLines([
						formatJsonLine('arguments', item.arguments),
						formatJsonLine('result', item.result),
						item.error ? formatJsonLine('error', item.error) : null
					])
				} satisfies ToolCallCell
			];
		case 'collabAgentToolCall':
			return [
				{
					id: item.id,
					kind: 'tool_call',
					timestamp,
					state: item.status === 'inProgress' ? 'running' : 'completed',
					pinned: false,
					label: 'Collaborator tool',
					command: item.tool,
					duration: 'sync',
					exitSummary: item.status,
					preview: `Sender ${item.senderThreadId} -> ${item.receiverThreadIds.join(', ') || 'pending receiver'}`,
					output: compactLines([
						item.prompt ? `prompt: ${item.prompt}` : null,
						Object.keys(item.agentsStates).length
							? `agents: ${JSON.stringify(item.agentsStates, null, 2)}`
							: null
					])
				} satisfies ToolCallCell
			];
		case 'webSearch':
			return [
				{
					id: item.id,
					kind: 'tool_call',
					timestamp,
					state: 'completed',
					pinned: false,
					label: 'Web search',
					command: item.query,
					duration: 'sync',
					exitSummary: item.action ? 'searched' : 'queued',
					preview: item.query,
					output: compactLines([
						item.action
							? `action: ${JSON.stringify(item.action, null, 2)}`
							: 'No action payload yet.'
					])
				} satisfies ToolCallCell
			];
		case 'imageView':
			return [
				{
					id: item.id,
					kind: 'tool_call',
					timestamp,
					state: 'completed',
					pinned: false,
					label: 'Image view',
					command: item.path,
					duration: 'sync',
					exitSummary: 'opened',
					preview: item.path,
					output: [item.path]
				} satisfies ToolCallCell
			];
		case 'enteredReviewMode':
			return [
				toAssistantMessageCell(item.id, timestamp, `Entered ${item.review}.`, `Review ${ordinal}`)
			];
		case 'exitedReviewMode':
			return [
				toAssistantMessageCell(item.id, timestamp, `Exited ${item.review}.`, `Review ${ordinal}`)
			];
		case 'contextCompaction':
			return [
				toAssistantMessageCell(
					item.id,
					timestamp,
					'Context compacted for continuity.',
					`Compaction ${ordinal}`
				)
			];
	}
}

function toAssistantMessageCell(
	id: string,
	timestamp: string,
	text: string,
	fallbackTitle: string
): AssistantMessageCell {
	const blocks = parseRichBlocks(text);
	const title = extractTitle(text, fallbackTitle);

	return {
		id,
		kind: 'assistant_message',
		timestamp,
		title,
		blocks
	};
}

function toCommandCell(
	item: Extract<ProtocolThreadItem, { type: 'commandExecution' }>,
	timestamp: string
): ToolCallCell {
	const state =
		item.status === 'inProgress' ? 'running' : item.status === 'completed' ? 'completed' : 'failed';
	const output = splitOutput(item.aggregatedOutput);

	return {
		id: item.id,
		kind: 'tool_call',
		timestamp,
		state,
		pinned: false,
		label: 'Command execution',
		command: item.command,
		duration: formatDuration(item.durationMs),
		exitSummary: formatExitSummary(item.status, item.exitCode),
		preview: output[0] ?? `${item.cwd} -> ${item.status}`,
		output: output.length ? output : ['No command output captured yet.']
	};
}

function toDiffCell(
	item: Extract<ProtocolThreadItem, { type: 'fileChange' }>,
	timestamp: string
): DiffCell {
	const previewFile = item.changes[0]?.path ?? 'workspace changes';
	const patch = item.changes
		.map((change) => change.diff || `${formatPatchChangeKind(change.kind)} ${change.path}`)
		.join('\n\n');

	return {
		id: item.id,
		kind: 'diff',
		timestamp,
		autoCompact: true,
		summary: item.status === 'inProgress' ? 'Pending file changes' : 'Workspace file changes',
		stats: `${item.changes.length} file${item.changes.length === 1 ? '' : 's'} · ${item.status}`,
		previewFile,
		patch: patch || 'No diff payload captured.'
	};
}

function summarizeUserInputs(content: ProtocolUserInput[]): string {
	return content
		.map((input) => {
			switch (input.type) {
				case 'text':
					return input.text;
				case 'image':
					return `[image] ${input.url}`;
				case 'localImage':
					return `[local image] ${input.path}`;
				case 'skill':
					return `[skill] ${input.name} (${input.path})`;
				case 'mention':
					return `[mention] ${input.name} (${input.path})`;
			}
		})
		.filter(Boolean)
		.join('\n\n');
}

function parseRichBlocks(text: string): RichBlock[] {
	const normalized = text.trim();
	if (!normalized) {
		return [{ type: 'paragraph', text: 'No message content captured.' }];
	}

	const blocks: RichBlock[] = [];
	const chunks = normalized.split(/\n\n+/);

	for (const chunk of chunks) {
		const trimmed = chunk.trim();
		if (!trimmed) {
			continue;
		}

		const codeMatch = trimmed.match(/^```([^\n]*)\n([\s\S]*)```$/);
		if (codeMatch) {
			blocks.push({
				type: 'code',
				language: codeMatch[1]?.trim() || 'text',
				text: codeMatch[2].trim()
			});
			continue;
		}

		const bulletLines = trimmed
			.split('\n')
			.map((line) => line.trim())
			.filter(Boolean);
		if (bulletLines.length && bulletLines.every((line) => /^[-*]\s+/.test(line))) {
			blocks.push({
				type: 'bullets',
				items: bulletLines.map((line) => line.replace(/^[-*]\s+/, ''))
			});
			continue;
		}

		blocks.push({ type: 'paragraph', text: trimmed });
	}

	return blocks;
}

function extractTitle(text: string, fallbackTitle: string): string {
	const firstLine = text
		.trim()
		.split('\n')
		.map((line) => line.trim())
		.find(Boolean);

	if (!firstLine) {
		return fallbackTitle;
	}

	const title = firstLine.replace(/^#{1,6}\s+/, '').replace(/^[-*]\s+/, '');
	return title.length <= 72 ? title : fallbackTitle;
}

function splitOutput(output: string | null): string[] {
	if (!output?.trim()) {
		return [];
	}

	return output
		.replace(/\r\n?/g, '\n')
		.trimEnd()
		.split('\n')
		.map(normalizeTerminalLine);
}

function formatDuration(durationMs: number | null): string {
	if (durationMs === null) {
		return 'live';
	}

	if (durationMs < 1000) {
		return `${durationMs}ms`;
	}

	return `${(durationMs / 1000).toFixed(durationMs >= 10000 ? 0 : 1)}s`;
}

function formatExitSummary(status: string, exitCode: number | null): string {
	if (status === 'inProgress') {
		return 'running';
	}

	if (exitCode !== null) {
		return `exit ${exitCode}`;
	}

	return status;
}

function formatTimestamp(unixSeconds: number): string {
	return new Intl.DateTimeFormat('en-US', {
		hour: 'numeric',
		minute: '2-digit'
	}).format(unixSeconds * 1000);
}

function mapToolStatus(status: string): ToolCallCell['state'] {
	if (status === 'inProgress') {
		return 'running';
	}

	if (status === 'failed' || status === 'error') {
		return 'failed';
	}

	return 'completed';
}

function summarizeMcpCall(item: Extract<ProtocolThreadItem, { type: 'mcpToolCall' }>): string {
	if (item.error) {
		return `${item.server}/${item.tool} failed`;
	}

	if (item.result) {
		return `${item.server}/${item.tool} returned a result`;
	}

	return `${item.server}/${item.tool} ${item.status}`;
}

function formatJsonLine(label: string, value: unknown): string {
	return `${label}: ${JSON.stringify(value, null, 2)}`;
}

function compactLines(lines: Array<string | null>): string[] {
	return lines
		.flatMap((line) => (line ? line.split('\n') : []))
		.map(normalizeTerminalLine)
		.filter(Boolean);
}

function normalizeTerminalLine(line: string) {
	return line
		.replace(ANSI_ESCAPE_PATTERN, '')
		.replace(/➜/g, '->')
		.replace(/…/g, '...');
}

function formatPatchChangeKind(
	kind: Extract<ProtocolThreadItem, { type: 'fileChange' }>['changes'][number]['kind']
) {
	if (kind.type === 'update' && kind.move_path) {
		return `move from ${kind.move_path}`;
	}

	return kind.type;
}
