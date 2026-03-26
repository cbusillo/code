import type { SessionSummary, TranscriptCell } from '$lib/transcript-model';

export const sessionRail: SessionSummary[] = [
	{
		id: 'sess_live_phone',
		title: 'Prototype the continuity-first WebUI',
		workspace: '~/Developer/code',
		device: 'Studio Mac',
		updated: 'live now',
		state: 'live-elsewhere',
		preview: 'Golden transcript fixture, cross-device handoff, and reasoning rules.'
	},
	{
		id: 'sess_waiting',
		title: 'Refresh upstream and preserve local fixes',
		workspace: '~/Developer/code-refresh-main',
		device: 'Remote Mac mini',
		updated: '18m ago',
		state: 'waiting',
		preview: 'Release binary validated, waiting for next decision.'
	},
	{
		id: 'sess_paused',
		title: 'Shared-session transport parity',
		workspace: '~/Developer/code',
		device: 'Desk TUI',
		updated: '2h ago',
		state: 'paused',
		preview: 'Binary smoke plus websocket parity validation.'
	},
	{
		id: 'sess_here',
		title: 'Books stack rollout follow-up',
		workspace: '~/Developer/claude-local-machine',
		device: 'This browser',
		updated: 'yesterday',
		state: 'live-here',
		preview: 'RMAB and Audiobookshelf routing notes.'
	}
];

export const transcriptCells: TranscriptCell[] = [
	{
		id: 'user_01',
		kind: 'user_message',
		timestamp: '12:14 PM',
		text: 'Build the cleanest EveryCode web UI you can. I want to start in the local TUI, then keep up from iPhone or iPad without losing the thread.'
	},
	{
		id: 'think_01',
		kind: 'thinking',
		state: 'completed',
		pinned: false,
		timestamp: '12:14 PM',
		duration: '8s',
		summary: 'Mapped the product around continuity, not generic chat.',
		content: [
			'The session runtime already exists. The web product should revolve around observing, continuing, or taking over that same thread from another device.',
			'The transcript needs explicit lifecycle rules so thinking, tool calls, and diffs do not compete for attention.'
		]
	},
	{
		id: 'tool_01',
		kind: 'tool_call',
		state: 'completed',
		pinned: false,
		timestamp: '12:15 PM',
		label: 'Shell command',
		command: 'rg -n "thread/read|thread/start|turn/start" code-rs/app-server src',
		duration: '0.4s',
		exitSummary: 'exit 0',
		preview:
			'Confirmed the browser MVP can be built on thread/list, read, start, resume, and turn/start.',
		output: [
			'code-rs/app-server/src/code_message_processor.rs:1642: async fn thread_list(...)',
			'code-rs/app-server/src/code_message_processor.rs:1768: async fn thread_read(...)',
			'code-rs/app-server/src/code_message_processor.rs:1838: async fn thread_start(...)',
			'code-rs/app-server/src/code_message_processor.rs:1929: async fn turn_start(...)',
			'code-rs/app-server/src/message_processor.rs:317: addConversationListener(...) bridges live turn updates',
			'docs/shared-session-web-ui-matrix.md:11: v1 browser client should stay on protocol-first primitives'
		]
	},
	{
		id: 'assistant_01',
		kind: 'assistant_message',
		timestamp: '12:16 PM',
		title: 'Direction',
		blocks: [
			{
				type: 'paragraph',
				text: 'The safest way to start over is to anchor the product in the transcript instead of the shell. That lets us solve your real fear: overlapping UI states that feel good in isolation but fight each other when combined.'
			},
			{
				type: 'bullets',
				items: [
					'Cross-device continuity is the top requirement.',
					'Thinking, tool calls, diffs, approvals, and questions need lifecycle rules.',
					'The shell should stay quiet so the transcript can carry the product.'
				]
			}
		]
	},
	{
		id: 'think_02',
		kind: 'thinking',
		state: 'active',
		pinned: false,
		timestamp: '12:18 PM',
		duration: 'live',
		summary: 'Drafting a transcript rule system.',
		content: [
			'Completed thinking should not vanish, but it also should not dominate the reading flow.',
			'Approval responses from a phone should be possible without necessarily taking over the entire session.'
		]
	},
	{
		id: 'tool_02',
		kind: 'tool_call',
		state: 'running',
		pinned: false,
		timestamp: '12:18 PM',
		label: 'Command execution',
		command: 'pnpm --dir every-code-webui dev --host 0.0.0.0 --port 4173',
		duration: '3.1s',
		exitSummary: 'running',
		preview: 'Starting the browser prototype locally.',
		output: [
			'> every-code-webui@0.0.1 dev /Users/cbusillo/Developer/code/every-code-webui',
			'> vite dev --host 0.0.0.0 --port 4173',
			'',
			'  VITE v7.3.1 ready in 691 ms',
			'  ➜ Local:   http://localhost:4173/',
			'  ➜ Network: http://192.168.1.44:4173/',
			'  waiting for browser session to reconnect...',
			'  hmr update /src/lib/components/transcript-prototype.svelte'
		]
	},
	{
		id: 'diff_01',
		kind: 'diff',
		timestamp: '12:19 PM',
		autoCompact: true,
		summary: 'Golden transcript prototype',
		stats: '4 files changed · +312 −9',
		previewFile: 'src/routes/+page.svelte',
		patch: `diff --git a/src/routes/+page.svelte b/src/routes/+page.svelte
index 4f9a0d2..7cf1c9b 100644
--- a/src/routes/+page.svelte
+++ b/src/routes/+page.svelte
@@
-<h1>Welcome to SvelteKit</h1>
-<p>Visit svelte.dev/docs/kit to read the documentation</p>
+<svelte:head>
+  <title>EveryCode WebUI Prototype</title>
+</svelte:head>
+
+<TranscriptPrototype />`
	},
	{
		id: 'diff_02',
		kind: 'diff',
		timestamp: '12:20 PM',
		autoCompact: true,
		summary: 'Continuity control states',
		stats: '2 files changed · +96 −14',
		previewFile: 'src/lib/components/transcript-prototype.svelte',
		patch: `diff --git a/src/lib/components/transcript-prototype.svelte b/src/lib/components/transcript-prototype.svelte
index 0f7817d..c4c82de 100644
--- a/src/lib/components/transcript-prototype.svelte
+++ b/src/lib/components/transcript-prototype.svelte
@@
-<button>Take over</button>
+<button data-mode="view-only">View only</button>
+<button data-mode="respond-remotely">Respond remotely</button>
+<button data-mode="take-over">Take over when current turn settles</button>

+{#if continuityMode === 'respond-remotely'}
+  <p>You can answer approvals and questions without stealing the keyboard.</p>
+{/if}`
	},
	{
		id: 'approval_01',
		kind: 'approval',
		status: 'pending',
		timestamp: '12:21 PM',
		label: 'Patch approval',
		detail:
			'Update the WebUI scaffold to replace the starter page with the golden transcript prototype.',
		primary: 'Apply patch',
		secondary: 'Hold for review'
	},
	{
		id: 'approval_02',
		kind: 'approval',
		status: 'answered',
		timestamp: '12:21 PM',
		device: 'iPhone 16 Pro',
		label: 'Shell access policy',
		detail:
			'Confirm that the phone UI may answer structured approvals without taking over the active shell session.',
		primary: 'Allow remote approvals',
		secondary: 'Keep approvals local only',
		resolution: 'Allow remote approvals',
		respondedBy: 'iPhone 16 Pro'
	},
	{
		id: 'question_01',
		kind: 'question',
		status: 'answered',
		timestamp: '12:22 PM',
		header: 'Reasoning behavior',
		prompt: 'How should completed reasoning behave by default?',
		options: [
			{ label: 'Collapse after a short delay', description: 'Keep history readable.' },
			{ label: 'Stay visible forever', description: 'Favor maximum context.' },
			{ label: 'Hide immediately', description: 'Minimize transcript noise.' }
		],
		allowOther: false,
		allowSecret: false,
		questionCountInGroup: 1,
		answer: 'Collapse after a short delay',
		respondedBy: 'iPhone 16 Pro'
	},
	{
		id: 'error_01',
		kind: 'error',
		status: 'interrupted',
		timestamp: '12:23 PM',
		message: 'Takeover was deferred while a turn was still running.',
		detail:
			'The UI should preserve intent here: keep the request visible and offer “Continue here” as soon as the active turn settles.'
	},
	{
		id: 'think_03',
		kind: 'thinking',
		state: 'completed',
		pinned: true,
		timestamp: '12:24 PM',
		duration: '14s',
		summary: 'Pinned reasoning',
		content: [
			'This one stays expanded to show how history inspection should work when the user wants to keep the internal reasoning context attached to a turn.',
			'Pinned state should always defeat timer-based collapse and fade behavior.'
		]
	}
];

export const selectedSession = sessionRail[0];
