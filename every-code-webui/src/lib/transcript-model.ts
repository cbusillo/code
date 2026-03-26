export type SessionState = 'live-here' | 'live-elsewhere' | 'paused' | 'waiting';

export type TranscriptSessionState =
	| 'read_only'
	| 'can_continue_here'
	| 'active_here'
	| 'active_elsewhere'
	| 'respond_remotely'
	| 'blocked_running';

export type TranscriptSessionBanner = {
	state: TranscriptSessionState;
	title: string;
	detail: string;
	primaryAction?: 'connect' | 'resume' | 'take_over';
	primaryLabel?: string;
};

export type SessionSummary = {
	id: string;
	title: string;
	workspace: string;
	device: string;
	updated: string;
	state: SessionState;
	preview: string;
};

export type RichBlock =
	| { type: 'paragraph'; text: string }
	| { type: 'bullets'; items: string[] }
	| { type: 'code'; language: string; text: string };

type BaseCell = {
	id: string;
	timestamp: string;
};

export type UserMessageCell = BaseCell & {
	kind: 'user_message';
	text: string;
};

export type AssistantMessageCell = BaseCell & {
	kind: 'assistant_message';
	title: string;
	blocks: RichBlock[];
};

export type ThinkingCell = BaseCell & {
	kind: 'thinking';
	state: 'active' | 'completed';
	pinned: boolean;
	duration: string;
	summary: string;
	content: string[];
};

export type ToolCallCell = BaseCell & {
	kind: 'tool_call';
	state: 'running' | 'completed' | 'failed';
	pinned: boolean;
	label: string;
	command: string;
	duration: string;
	exitSummary: string;
	preview: string;
	output: string[];
};

export type DiffCell = BaseCell & {
	kind: 'diff';
	autoCompact: boolean;
	summary: string;
	stats: string;
	previewFile: string;
	patch: string;
};

export type ApprovalCell = BaseCell & {
	kind: 'approval';
	status: 'pending' | 'answered';
	device?: string;
	label: string;
	detail: string;
	primary: string;
	secondary: string;
	resolution?: string;
	respondedBy?: string;
};

export type QuestionCell = BaseCell & {
	kind: 'question';
	status: 'pending' | 'answered';
	header: string;
	prompt: string;
	options: Array<{ label: string; description: string }>;
	allowOther: boolean;
	allowSecret: boolean;
	questionCountInGroup: number;
	answer?: string;
	respondedBy?: string;
};

export type ErrorCell = BaseCell & {
	kind: 'error';
	status: 'interrupted' | 'failed';
	message: string;
	detail: string;
};

export type TranscriptCell =
	| UserMessageCell
	| AssistantMessageCell
	| ThinkingCell
	| ToolCallCell
	| DiffCell
	| ApprovalCell
	| QuestionCell
	| ErrorCell;
