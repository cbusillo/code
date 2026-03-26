import { describe, expect, it } from 'vitest';

import { protocolThreadToTranscriptCells, structuredRequestsToTranscriptCells } from '$lib/protocol-transcript';
import type { SharedSessionQuestionRequest } from '$lib/shared-session-domain';
import type { ProtocolThread, ProtocolThreadItem, ProtocolTurn } from '$lib/shared-session-types';

describe('protocolThreadToTranscriptCells', () => {
	it('maps core protocol items into the transcript cell model', () => {
		const cells = protocolThreadToTranscriptCells(
			makeThread('thread_cells', [
				makeTurn('turn_one', 'completed', [
					userMessage('user_1', 'Hydrate the real transcript'),
					reasoning(
						'reason_1',
						['Map protocol items'],
						['Keep the browser and TUI in one transcript model.']
					),
					commandExecution('cmd_1', 'pnpm check', 'completed', 'All checks passed', 0, 4200),
					fileChange('diff_1', 'completed', [
						{
							path: 'src/routes/+page.svelte',
							kind: { type: 'update', move_path: null },
							diff: '@@\n-raw cards\n+real transcript'
						}
					]),
					agentMessage(
						'assistant_1',
						'Direction\n\n- unify the transcript\n- keep the fixture intact'
					)
				])
			])
		);

		expect(cells.map((cell) => cell.kind)).toEqual([
			'user_message',
			'thinking',
			'tool_call',
			'diff',
			'assistant_message'
		]);
		expect(cells[0]).toMatchObject({ kind: 'user_message', text: 'Hydrate the real transcript' });
		expect(cells[1]).toMatchObject({ kind: 'thinking', state: 'completed' });
		expect(cells[2]).toMatchObject({ kind: 'tool_call', exitSummary: 'exit 0' });
		expect(cells[3]).toMatchObject({ kind: 'diff', previewFile: 'src/routes/+page.svelte' });
		expect(cells[4]).toMatchObject({ kind: 'assistant_message', title: 'Direction' });
	});

	it('adds failed turn details as an error cell', () => {
		const cells = protocolThreadToTranscriptCells(
			makeThread('thread_failed', [
				{
					id: 'turn_failed',
					status: 'failed',
					items: [userMessage('user_2', 'Retry the bridge')],
					error: {
						message: 'Bridge attach failed',
						codexErrorInfo: null,
						additionalDetails: 'The websocket closed before the live listener attached.'
					}
				}
			])
		);

		expect(cells.at(-1)).toMatchObject({
			kind: 'error',
			status: 'failed',
			message: 'Bridge attach failed'
		});
	});

	it('normalizes command output for transcript readability', () => {
		const cells = protocolThreadToTranscriptCells(
			makeThread('thread_terminal', [
				makeTurn('turn_terminal', 'completed', [
					commandExecution(
						'cmd_terminal',
						'pnpm dev',
						'completed',
						'\u001b[32mready\u001b[39m\n➜ Local: http://localhost:4173/\nwaiting…',
						0,
						800
					)
				])
			])
		);

		expect(cells[0]).toMatchObject({
			kind: 'tool_call',
			output: ['ready', '-> Local: http://localhost:4173/', 'waiting...']
		});
	});

	it('maps structured question requests with rich transcript metadata', () => {
		const requests: SharedSessionQuestionRequest[] = [
			makeQuestionRequest('question_1', 'req_1', 'Pick a scope'),
			makeQuestionRequest('question_2', 'req_1', 'Pick a renderer'),
			makeQuestionRequest('question_3', 'req_2', 'Name the prototype')
		];

		const cells = structuredRequestsToTranscriptCells(requests);
		const first = cells[0];
		const third = cells[2];

		expect(first).toMatchObject({
			kind: 'question',
			header: 'Implementation choices',
			allowOther: true,
			questionCountInGroup: 2
		});
		expect(first.kind === 'question' ? first.options[0] : null).toEqual({
			label: 'Use the transcript',
			description: 'Keep the transcript primary.'
		});
		expect(third).toMatchObject({
			kind: 'question',
			questionCountInGroup: 1
		});
	});
});

function makeThread(id: string, turns: ProtocolTurn[]): ProtocolThread {
	return {
		id,
		preview: `Preview for ${id}`,
		modelProvider: 'openai',
		createdAt: 1,
		updatedAt: 2,
		path: `/tmp/${id}.jsonl`,
		cwd: '/Users/cbusillo/Developer/code/every-code-webui',
		cliVersion: '0.0.0',
		source: 'cli',
		gitInfo: null,
		turns
	};
}

function makeTurn(
	id: string,
	status: ProtocolTurn['status'],
	items: ProtocolThreadItem[]
): ProtocolTurn {
	return {
		id,
		status,
		items,
		error: null
	};
}

function userMessage(
	id: string,
	text: string
): Extract<ProtocolThreadItem, { type: 'userMessage' }> {
	return {
		type: 'userMessage',
		id,
		content: [{ type: 'text', text, text_elements: [] }]
	};
}

function agentMessage(
	id: string,
	text: string
): Extract<ProtocolThreadItem, { type: 'agentMessage' }> {
	return {
		type: 'agentMessage',
		id,
		text
	};
}

function reasoning(
	id: string,
	summary: string[],
	content: string[]
): Extract<ProtocolThreadItem, { type: 'reasoning' }> {
	return {
		type: 'reasoning',
		id,
		summary,
		content
	};
}

function commandExecution(
	id: string,
	command: string,
	status: Extract<ProtocolThreadItem, { type: 'commandExecution' }>['status'],
	aggregatedOutput: string,
	exitCode: number,
	durationMs: number
): Extract<ProtocolThreadItem, { type: 'commandExecution' }> {
	return {
		type: 'commandExecution',
		id,
		command,
		cwd: '/Users/cbusillo/Developer/code/every-code-webui',
		processId: null,
		status,
		commandActions: [],
		aggregatedOutput,
		exitCode,
		durationMs
	};
}

function fileChange(
	id: string,
	status: Extract<ProtocolThreadItem, { type: 'fileChange' }>['status'],
	changes: Extract<ProtocolThreadItem, { type: 'fileChange' }>['changes']
): Extract<ProtocolThreadItem, { type: 'fileChange' }> {
	return {
		type: 'fileChange',
		id,
		changes,
		status
	};
}

function makeQuestionRequest(
	id: string,
	requestId: string,
	prompt: string
): SharedSessionQuestionRequest {
	return {
		kind: 'question',
		id,
		requestId,
		threadId: 'thread_1',
		turnId: 'turn_1',
		itemId: 'item_1',
		questionId: `${id}_id`,
		status: 'pending',
		header: 'Implementation choices',
		prompt,
		options: [{ label: 'Use the transcript', description: 'Keep the transcript primary.' }],
		allowOther: true,
		allowSecret: false
	};
}
