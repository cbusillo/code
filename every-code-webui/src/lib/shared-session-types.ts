import type { RequestId as JsonRpcRequestId } from '../../../code-rs/app-server-protocol/schema/typescript/RequestId';
import type { ServerRequest as ProtocolServerRequest } from '../../../code-rs/app-server-protocol/schema/typescript/ServerRequest';

export type { AddConversationSubscriptionResponse as ConversationListenerResult } from '../../../code-rs/app-server-protocol/schema/typescript/AddConversationSubscriptionResponse';
export type { InitializeResponse as InitializeResult } from '../../../code-rs/app-server-protocol/schema/typescript/InitializeResponse';
export type { RequestId as JsonRpcRequestId } from '../../../code-rs/app-server-protocol/schema/typescript/RequestId';
export type { ServerRequest as ProtocolServerRequest } from '../../../code-rs/app-server-protocol/schema/typescript/ServerRequest';
export type { CommandExecutionApprovalDecision as ProtocolCommandExecutionApprovalDecision } from '../../../code-rs/app-server-protocol/schema/typescript/v2/CommandExecutionApprovalDecision';
export type { CommandExecutionRequestApprovalParams as ProtocolCommandExecutionRequestApprovalParams } from '../../../code-rs/app-server-protocol/schema/typescript/v2/CommandExecutionRequestApprovalParams';
export type { CommandExecutionRequestApprovalResponse as ProtocolCommandExecutionRequestApprovalResponse } from '../../../code-rs/app-server-protocol/schema/typescript/v2/CommandExecutionRequestApprovalResponse';
export type { FileChangeApprovalDecision as ProtocolFileChangeApprovalDecision } from '../../../code-rs/app-server-protocol/schema/typescript/v2/FileChangeApprovalDecision';
export type { FileChangeRequestApprovalParams as ProtocolFileChangeRequestApprovalParams } from '../../../code-rs/app-server-protocol/schema/typescript/v2/FileChangeRequestApprovalParams';
export type { FileChangeRequestApprovalResponse as ProtocolFileChangeRequestApprovalResponse } from '../../../code-rs/app-server-protocol/schema/typescript/v2/FileChangeRequestApprovalResponse';
export type { FileUpdateChange as ProtocolFileUpdateChange } from '../../../code-rs/app-server-protocol/schema/typescript/v2/FileUpdateChange';
export type { GitInfo as ProtocolGitInfo } from '../../../code-rs/app-server-protocol/schema/typescript/v2/GitInfo';
export type { Personality as ProtocolPersonality } from '../../../code-rs/app-server-protocol/schema/typescript/Personality';
export type { SessionSource as ProtocolSessionSource } from '../../../code-rs/app-server-protocol/schema/typescript/v2/SessionSource';
export type { Thread as ProtocolThread } from '../../../code-rs/app-server-protocol/schema/typescript/v2/Thread';
export type { ThreadForkResponse as ThreadForkResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadForkResponse';
export type { ThreadItem as ProtocolThreadItem } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadItem';
export type { ThreadLoadedListResponse as ThreadLoadedListResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadLoadedListResponse';
export type { ThreadListResponse as ThreadListResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadListResponse';
export type { ThreadReadResponse as ThreadReadResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadReadResponse';
export type { ThreadResumeResponse as ThreadResumeResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadResumeResponse';
export type { ThreadStartResponse as ThreadStartResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ThreadStartResponse';
export type { ToolRequestUserInputAnswer as ProtocolToolRequestUserInputAnswer } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ToolRequestUserInputAnswer';
export type { ToolRequestUserInputParams as ProtocolToolRequestUserInputParams } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ToolRequestUserInputParams';
export type { ToolRequestUserInputQuestion as ProtocolToolRequestUserInputQuestion } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ToolRequestUserInputQuestion';
export type { ToolRequestUserInputResponse as ProtocolToolRequestUserInputResponse } from '../../../code-rs/app-server-protocol/schema/typescript/v2/ToolRequestUserInputResponse';
export type { Turn as ProtocolTurn } from '../../../code-rs/app-server-protocol/schema/typescript/v2/Turn';
export type { TurnError as ProtocolTurnError } from '../../../code-rs/app-server-protocol/schema/typescript/v2/TurnError';
export type { TurnInterruptResponse as TurnInterruptResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/TurnInterruptResponse';
export type { TurnSteerResponse as TurnSteerResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/TurnSteerResponse';
export type { TurnStartParams as ProtocolTurnStartParams } from '../../../code-rs/app-server-protocol/schema/typescript/v2/TurnStartParams';
export type { TurnStartResponse as TurnStartResult } from '../../../code-rs/app-server-protocol/schema/typescript/v2/TurnStartResponse';
export type { UserInput as ProtocolUserInput } from '../../../code-rs/app-server-protocol/schema/typescript/v2/UserInput';

export type JsonRpcSuccess<T> = {
	jsonrpc: '2.0';
	id: number;
	result: T;
};

export type JsonRpcFailure = {
	jsonrpc: '2.0';
	id: number;
	error: {
		code: number;
		message: string;
		data?: unknown;
	};
};

export type JsonRpcNotification = {
	jsonrpc: '2.0';
	method: string;
	params?: unknown;
};

export type JsonRpcServerRequest = {
	jsonrpc: '2.0';
	id: JsonRpcRequestId;
	method: ProtocolServerRequest['method'];
	params: ProtocolServerRequest['params'];
};
