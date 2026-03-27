<script lang="ts">
	import { browser } from '$app/environment';
	import { onDestroy, onMount } from 'svelte';

	import TranscriptThreadView from '$lib/components/transcript-thread-view.svelte';
	import {
		assessWebSocketConnection,
		type ConnectionAssessmentTone
	} from '$lib/shared-session-connection-policy';
	import {
		SharedSessionDomain,
		type SharedSessionApprovalRequest,
		type SharedSessionQuestionRequest
	} from '$lib/shared-session-domain';
	import {
		protocolThreadToTranscriptCells,
		structuredRequestsToTranscriptCells
	} from '$lib/protocol-transcript';
	import {
		EMPTY_PHASE3_LIBRARY,
		appendCheckpoint,
		appendHandoff,
		appendReviewExport,
		buildExternalHandoffPrompt,
		buildReviewExportPayload,
		detectExternalHandoffSource,
		loadPhase3Library,
		persistPhase3Library,
		type Phase3Library
	} from '$lib/shared-session-phase3';
	import { deriveTranscriptSessionBanner } from '$lib/transcript-session-state';
	import {
		clearResponderDrafts,
		loadResponderDrafts,
		persistResponderDrafts,
		responderDraftKey,
		type ResponderDraftMap
	} from '$lib/responder-drafts';
	import {
		canSubmitQuestionGroup,
		groupedQuestionBatches,
		groupPendingQuestionRequests,
		questionAnswerList,
		questionDraftsByCellId
	} from '$lib/shared-session-question-state';
	import { buildPendingThreadInbox, type PendingThreadInboxEntry } from '$lib/shared-session-inbox';
	import {
		buildThreadNavigationEntries,
		emptyThreadNavigationState,
		loadThreadNavigationState,
		markThreadOpened,
		markThreadSeen,
		persistThreadNavigationState,
		recentThreadEntries,
		togglePinnedThread,
		type ThreadNavigationEntry,
		type ThreadNavigationState
	} from '$lib/shared-session-thread-navigation';
	import {
		appendVoiceTranscript,
		supportsVoiceInput,
		voiceCaptureStatusLabel,
		type VoiceCaptureState
	} from '$lib/shared-session-voice';
	import {
		buildTranscriptReviewArtifacts,
		loadReviewNotes
	} from '$lib/transcript-review-artifacts';
	import type { ProtocolPersonality, ProtocolThread } from '$lib/shared-session-types';

	const NOTIFICATION_SETTINGS_KEY = 'every-code-webui.notifications-enabled';
	const KEEP_AWAKE_SETTINGS_KEY = 'every-code-webui.keep-awake';
	const CONNECTION_ACKNOWLEDGEMENT_KEY = 'every-code-webui.connection-acknowledgement';

	type NotificationFocusTarget = 'pending' | 'composer' | 'thread';

	type BrowserInstallPromptEvent = Event & {
		prompt: () => Promise<void>;
		userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
	};

	type BrowserWakeLockSentinel = {
		released: boolean;
		release: () => Promise<void>;
	};

	type BrowserSpeechRecognitionResult = {
		isFinal: boolean;
		0: { transcript: string };
	};

	type BrowserSpeechRecognitionEvent = {
		resultIndex: number;
		results: ArrayLike<BrowserSpeechRecognitionResult>;
	};

	type BrowserSpeechRecognitionErrorEvent = {
		error: string;
		message?: string;
	};

	type BrowserSpeechRecognition = {
		continuous: boolean;
		interimResults: boolean;
		lang: string;
		onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null;
		onerror: ((event: BrowserSpeechRecognitionErrorEvent) => void) | null;
		onend: (() => void) | null;
		start: () => void;
		stop: () => void;
	};

	type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition;

	const domain = new SharedSessionDomain();
	const workspaceState = domain.store;

	let wsUrl = $state(domain.getSnapshot().wsUrl);
	let startCwd = $state(domain.getSnapshot().startCwd);
	let startPrompt = $state(domain.getSnapshot().startPrompt);
	let composerText = $state(domain.getSnapshot().composerText);
	let turnOverrideModel = $state(domain.getSnapshot().turnOverrideModel);
	let turnOverrideCwd = $state(domain.getSnapshot().turnOverrideCwd);
	let turnOverridePersonality = $state<'' | ProtocolPersonality>(
		domain.getSnapshot().turnOverridePersonality
	);
	let steerText = $state('');
	let checkpointLabel = $state('');
	let checkpointNote = $state('');
	let externalHandoffUrl = $state('');
	let phase3Library = $state<Phase3Library>(
		browser ? loadPhase3Library(window.localStorage) : EMPTY_PHASE3_LIBRARY
	);
	let questionDrafts = $state<ResponderDraftMap>(
		browser ? loadResponderDrafts(window.localStorage) : {}
	);
	let pendingActionsSection = $state<HTMLElement | null>(null);
	let responderSection = $state<HTMLElement | null>(null);
	let composerSection = $state<HTMLElement | null>(null);
	let threadRailSection = $state<HTMLElement | null>(null);
	let companionToolsSection = $state<HTMLDetailsElement | null>(null);
	let focusedQuestionRequestId = $state<string | null>(null);
	let showConnectionSettings = $state(domain.getSnapshot().connectionPhase !== 'connected');
	let threadSearch = $state('');
	let toastMessage = $state('');
	let showToast = $state(false);
	let threadNavigationState = $state<ThreadNavigationState>(
		browser ? loadThreadNavigationState(window.localStorage) : emptyThreadNavigationState()
	);
	let notificationsEnabled = $state(
		browser ? window.localStorage.getItem(NOTIFICATION_SETTINGS_KEY) === 'true' : false
	);
	let notificationPermission = $state<'default' | 'denied' | 'granted'>(
		browser && 'Notification' in window ? Notification.permission : 'default'
	);
	let keepAwakeEnabled = $state(
		browser ? window.localStorage.getItem(KEEP_AWAKE_SETTINGS_KEY) === 'true' : false
	);
	let wakeLockStatus = $state<'unsupported' | 'idle' | 'active' | 'error'>(
		browser && 'wakeLock' in navigator ? 'idle' : 'unsupported'
	);
	let wakeLockErrorMessage = $state('');
	let isOnline = $state(browser ? navigator.onLine : true);
	let pageVisibility = $state<'visible' | 'hidden' | 'prerender'>(
		browser ? (document.visibilityState as 'visible' | 'hidden' | 'prerender') : 'visible'
	);
	let voiceCaptureState = $state<VoiceCaptureState>(
		browser && supportsVoiceInput(window) ? 'idle' : 'unsupported'
	);
	let acknowledgedConnectionKey = $state(
		browser ? window.localStorage.getItem(CONNECTION_ACKNOWLEDGEMENT_KEY) ?? '' : ''
	);
	let voiceErrorMessage = $state('');
	let interimVoiceTranscript = $state('');
	let deferredInstallPrompt = $state<BrowserInstallPromptEvent | null>(null);
	let installMessage = $state('');
	let activityFilter = $state<'all' | 'selected' | 'blockers'>('all');
	let requestedThreadId = $state('');
	let requestedFocus = $state<NotificationFocusTarget>('thread');
	let attemptedDeepLinkConnect = $state(false);
	let handledRequestedThreadId = $state('');
	let wakeLockSentinel: BrowserWakeLockSentinel | null = null;
	let voiceRecognition: BrowserSpeechRecognition | null = null;

	const unsubscribe = workspaceState.subscribe((state) => {
		wsUrl = state.wsUrl;
		startCwd = state.startCwd;
		startPrompt = state.startPrompt;
		composerText = state.composerText;
		turnOverrideModel = state.turnOverrideModel;
		turnOverrideCwd = state.turnOverrideCwd;
		turnOverridePersonality = state.turnOverridePersonality;
	});

	$effect(() => {
		if (!browser) {
			return;
		}

		persistResponderDrafts(window.localStorage, questionDrafts);
	});

	$effect(() => {
		if (!browser) {
			return;
		}

		persistPhase3Library(window.localStorage, phase3Library);
	});

	onMount(() => {
		if (!browser) {
			return;
		}

		const searchParams = new URLSearchParams(window.location.search);
		requestedThreadId = searchParams.get('thread') ?? '';
		requestedFocus = normalizeRequestedFocus(searchParams.get('focus'));

		const handleBeforeInstallPrompt = (event: Event) => {
			event.preventDefault();
			deferredInstallPrompt = event as BrowserInstallPromptEvent;
			installMessage = 'Install the companion for faster home-screen launch and better mobile continuity.';
		};

		const handleAppInstalled = () => {
			deferredInstallPrompt = null;
			installMessage = 'Companion installed on this device.';
		};

		const handleOnline = () => {
			isOnline = true;
			if ($workspaceState.connectionPhase === 'connected') {
				void domain.refreshForForeground();
			}
		};

		const handleOffline = () => {
			isOnline = false;
		};

		const handleVisibilityChange = () => {
			pageVisibility = document.visibilityState as 'visible' | 'hidden' | 'prerender';
			if (pageVisibility === 'visible' && $workspaceState.connectionPhase === 'connected') {
				void domain.refreshForForeground().then(() => focusRequestedThreadTarget());
			}
		};

		window.addEventListener('beforeinstallprompt', handleBeforeInstallPrompt);
		window.addEventListener('appinstalled', handleAppInstalled);
		window.addEventListener('online', handleOnline);
		window.addEventListener('offline', handleOffline);
		document.addEventListener('visibilitychange', handleVisibilityChange);

		return () => {
			window.removeEventListener('beforeinstallprompt', handleBeforeInstallPrompt);
			window.removeEventListener('appinstalled', handleAppInstalled);
			window.removeEventListener('online', handleOnline);
			window.removeEventListener('offline', handleOffline);
			document.removeEventListener('visibilitychange', handleVisibilityChange);
		};
	});

	$effect(() => {
		if (!browser) {
			return;
		}

		persistThreadNavigationState(window.localStorage, threadNavigationState);
		window.localStorage.setItem(NOTIFICATION_SETTINGS_KEY, String(notificationsEnabled));
		window.localStorage.setItem(KEEP_AWAKE_SETTINGS_KEY, String(keepAwakeEnabled));
		if (acknowledgedConnectionKey) {
			window.localStorage.setItem(CONNECTION_ACKNOWLEDGEMENT_KEY, acknowledgedConnectionKey);
		} else {
			window.localStorage.removeItem(CONNECTION_ACKNOWLEDGEMENT_KEY);
		}
		notificationPermission = 'Notification' in window ? Notification.permission : 'default';
	});

	$effect(() => {
		const answeredQuestionKeys = $workspaceState.structuredRequests
			.filter(
				(request): request is SharedSessionQuestionRequest =>
					request.kind === 'question' && request.status === 'answered'
			)
			.map((request) => responderDraftKey(request));

		if (!answeredQuestionKeys.length) {
			return;
		}

		if (!answeredQuestionKeys.some((key) => key in questionDrafts)) {
			return;
		}

		questionDrafts = clearResponderDrafts(questionDrafts, answeredQuestionKeys);
	});

	$effect(() => {
		if ($workspaceState.connectionPhase === 'connected') {
			showConnectionSettings = false;
			return;
		}

		if ($workspaceState.connectionPhase === 'error' || $workspaceState.connectionPhase === 'offline') {
			showConnectionSettings = true;
		}
	});

	$effect(() => {
		if (!browser || !requestedThreadId || attemptedDeepLinkConnect) {
			return;
		}

		if ($workspaceState.connectionPhase !== 'offline' || !wsUrl.trim()) {
			return;
		}

		attemptedDeepLinkConnect = true;
		void connect();
	});

	$effect(() => {
		if (
			!browser ||
			!requestedThreadId ||
			handledRequestedThreadId === requestedThreadId ||
			$workspaceState.connectionPhase !== 'connected' ||
			!$workspaceState.threads.some((thread) => thread.id === requestedThreadId)
		) {
			return;
		}

		handledRequestedThreadId = requestedThreadId;
		void openThread(requestedThreadId).then(() => focusRequestedThreadTarget());
	});

	$effect(() => {
		if (!browser || wakeLockStatus === 'unsupported') {
			return;
		}

		if (
			keepAwakeEnabled &&
			selectedActiveTurn &&
			pageVisibility === 'visible' &&
			$workspaceState.connectionPhase === 'connected'
		) {
			void acquireWakeLock();
			return;
		}

		void releaseWakeLock();
	});

	onDestroy(() => {
		stopVoiceCapture();
		void releaseWakeLock();
		if (toastTimeout) {
			clearTimeout(toastTimeout);
		}
		unsubscribe();
		domain.disconnect('Workspace closed.');
	});

	const selectedThread = $derived.by(() => {
		if (
			$workspaceState.hydratedThread &&
			$workspaceState.hydratedThread.id === $workspaceState.selectedThreadId
		) {
			return $workspaceState.hydratedThread;
		}

		return (
			$workspaceState.threads.find((thread) => thread.id === $workspaceState.selectedThreadId) ??
			null
		);
	});

	const selectedRequests = $derived.by(() =>
		selectedThread
			? $workspaceState.structuredRequests.filter(
					(request) => request.threadId === selectedThread.id
				)
			: []
	);
	const transcriptCells = $derived.by(() => {
		if (!selectedThread) {
			return [];
		}

		return [
			...protocolThreadToTranscriptCells(selectedThread),
			...structuredRequestsToTranscriptCells(selectedRequests)
		];
	});
	const reviewArtifacts = $derived.by(() => buildTranscriptReviewArtifacts(transcriptCells));
	const selectedReviewNotes = $derived.by(() => {
		if (!browser || !selectedThread) {
			return {};
		}

		return loadReviewNotes(window.localStorage, selectedThread.id);
	});
	const transcriptBadges = $derived.by(() => {
		if (!selectedThread) {
			return [];
		}

		const pendingCount = selectedRequests.filter((request) => request.status === 'pending').length;
		const branchLabel = selectedThread.gitInfo?.branch?.trim();
		const contextLabel = compactThreadPath(selectedThread.cwd);

		return [
			`${selectedThread.turns.length} turn${selectedThread.turns.length === 1 ? '' : 's'}`,
			branchLabel ? `branch ${branchLabel}` : null,
			pendingCount > 0 ? `${pendingCount} pending action${pendingCount === 1 ? '' : 's'}` : null,
			contextLabel
		].filter((badge): badge is string => Boolean(badge));
	});
	const selectedThreadId = $derived.by(() => selectedThread?.id ?? null);
	const sessionBanner = $derived.by(() => {
		const selectedThreadLoadedElsewhere = selectedThreadId
			? $workspaceState.loadedThreadIds.includes(selectedThreadId) &&
				!$workspaceState.attachedThreadIds.includes(selectedThreadId)
			: false;

		return deriveTranscriptSessionBanner({
			connectionPhase: $workspaceState.connectionPhase,
			statusMessage: $workspaceState.statusMessage,
			selectedThread,
			activeThreadId: $workspaceState.activeThreadId,
			loadedElsewhere: selectedThreadLoadedElsewhere,
			takeoverRequestedThreadId: $workspaceState.takeoverRequestedThreadId,
			pendingAction: $workspaceState.pendingAction,
			structuredRequests: selectedRequests
		});
	});
	const pendingApprovals = $derived.by(() =>
		selectedRequests.filter(
			(request): request is SharedSessionApprovalRequest =>
				request.kind !== 'question' && request.status === 'pending'
		)
	);
	const filteredThreads = $derived.by(() => {
		const query = threadSearch.trim().toLowerCase();
		if (!query) {
			return threadEntries;
		}

		return threadEntries.filter((entry) => {
			const haystack = [
				entry.thread.preview,
				entry.thread.id,
				entry.thread.cwd,
				entry.thread.gitInfo?.branch,
				entry.thread.modelProvider
			]
				.filter(Boolean)
				.join(' ')
				.toLowerCase();

			return haystack.includes(query);
		});
	});
	const threadEntries = $derived.by(() =>
		buildThreadNavigationEntries(
			$workspaceState.threads,
			$workspaceState.structuredRequests,
			threadNavigationState,
			$workspaceState.selectedThreadId,
			$workspaceState.activeThreadId,
			$workspaceState.loadedThreadIds,
			$workspaceState.attachedThreadIds
		)
	);
	const recentThreads = $derived.by(() => {
		if (threadSearch.trim()) {
			return [];
		}

		return recentThreadEntries(
			threadEntries.filter((entry) => !entry.isSelected),
			3
		);
	});
	const unreadThreadCount = $derived.by(
		() => threadEntries.filter((entry) => entry.isUnread).length
	);
	const filteredEvents = $derived.by(() => {
		if (activityFilter === 'selected') {
			return $workspaceState.events.filter((event) => event.threadId === selectedThread?.id);
		}

		if (activityFilter === 'blockers') {
			return $workspaceState.events.filter(
				(event) =>
					event.method.includes('requestApproval') ||
					event.method.includes('requestUserInput') ||
					event.method.includes('interrupted') ||
					event.method.includes('failed')
			);
		}

		return $workspaceState.events;
	});
	const globalPendingCount = $derived.by(() =>
		$workspaceState.structuredRequests.filter((request) => request.status === 'pending').length
	);
	const inboxThreads = $derived.by(() =>
		buildPendingThreadInbox(
			$workspaceState.threads,
			$workspaceState.structuredRequests,
			$workspaceState.selectedThreadId,
			$workspaceState.activeThreadId
		)
	);
	const globalApprovalCount = $derived.by(
		() =>
			$workspaceState.structuredRequests.filter(
				(request) => request.kind !== 'question' && request.status === 'pending'
			).length
	);
	const globalQuestionSetCount = $derived.by(
		() => groupPendingQuestionRequests($workspaceState.structuredRequests).length
	);
	const selectedActiveTurn = $derived.by(() => {
		if (!selectedThread) {
			return null;
		}

		return [...selectedThread.turns].reverse().find((turn) => turn.status === 'inProgress') ?? null;
	});
	const selectedRunningElsewhere = $derived.by(
		() => Boolean(selectedActiveTurn && $workspaceState.activeThreadId !== selectedThread?.id)
	);
	const selectedTakeoverQueued = $derived.by(
		() => selectedThread?.id === $workspaceState.takeoverRequestedThreadId
	);
	const selectedLoadedElsewhere = $derived.by(() => {
		if (!selectedThreadId) {
			return false;
		}

		return (
			$workspaceState.loadedThreadIds.includes(selectedThreadId) &&
			!$workspaceState.attachedThreadIds.includes(selectedThreadId)
		);
	});
	const composerBlockedReason = $derived.by(() => {
		if ($workspaceState.connectionPhase !== 'connected') {
			return 'Connect and restore live updates before composing here.';
		}

		if (selectedLoadedElsewhere) {
			return 'Another window still has this thread attached. Bring it here before sending the next prompt from this browser.';
		}

		if (selectedTakeoverQueued) {
			return 'Takeover is queued for this browser. Live updates will attach here automatically when the active turn settles.';
		}

		if (selectedRunningElsewhere) {
			return 'Another window is still driving this turn. Wait for it to settle or bring it here first.';
		}

		return '';
	});
	const pendingQuestionGroups = $derived.by(() => {
		return groupPendingQuestionRequests(selectedRequests);
	});
	const groupedQuestionBatchesList = $derived.by(() => groupedQuestionBatches(pendingQuestionGroups));
	const mobilePrimaryLabel = $derived.by(() => {
		if (selectedActiveTurn) {
			return 'Stop turn';
		}

		if (selectedTakeoverQueued) {
			return 'Takeover queued';
		}

		if (sessionBanner?.primaryLabel) {
			return sessionBanner.primaryLabel;
		}

		return selectedThread ? 'Jump to composer' : '';
	});
	const questionDraftsById = $derived.by(() =>
		questionDraftsByCellId(
			selectedRequests.filter(
				(request): request is SharedSessionQuestionRequest => request.kind === 'question'
			),
			questionDrafts
		)
	);
	const connectionAssessment = $derived.by(() => assessWebSocketConnection(wsUrl));
	const connectionAcknowledged = $derived.by(
		() =>
			Boolean(connectionAssessment.acknowledgementKey) &&
			acknowledgedConnectionKey === connectionAssessment.acknowledgementKey
	);
	const connectBlockedByAcknowledgement = $derived.by(
		() => connectionAssessment.requiresAcknowledgement && !connectionAcknowledged
	);
	const persistentRemoteBoundaryNotice = $derived.by(() => {
		if (
			$workspaceState.connectionPhase !== 'connected' ||
			!connectionAssessment.requiresAcknowledgement
		) {
			return null;
		}

		return `${connectionAssessment.guide.title}: ${connectionAssessment.guide.identityBoundary}`;
	});

	let previousPendingCount = 0;
	let previousNotificationPendingCount = 0;
	let seededCompletionNotifications = false;
	const notifiedCompletionIds = new Set<string>();
	let toastTimeout: ReturnType<typeof setTimeout> | null = null;
	let lastSelectedThreadId = $state('');

	function showToastMessage(message: string) {
		toastMessage = message;
		showToast = true;
		if (toastTimeout) {
			clearTimeout(toastTimeout);
		}
		toastTimeout = setTimeout(() => {
			showToast = false;
		}, 4000);
	}

	$effect(() => {
		if (!selectedThread?.id || selectedThread.id === lastSelectedThreadId) {
			return;
		}

		lastSelectedThreadId = selectedThread.id;
		threadNavigationState = markThreadOpened(threadNavigationState, selectedThread.id);
	});

	$effect(() => {
		if (!selectedThread) {
			return;
		}

		threadNavigationState = markThreadSeen(threadNavigationState, selectedThread);
	});

	$effect(() => {
		if (!browser) {
			return;
		}

		if (globalPendingCount > previousPendingCount) {
			toastMessage =
				globalPendingCount === 1
					? 'A new approval or question needs attention.'
					: `${globalPendingCount} approvals or questions need attention.`;
			showToast = true;
			if (toastTimeout) {
				clearTimeout(toastTimeout);
			}
			toastTimeout = setTimeout(() => {
				showToast = false;
			}, 4000);
		}

		previousPendingCount = globalPendingCount;
	});

	$effect(() => {
		if (!browser || !notificationsEnabled || notificationPermission !== 'granted') {
			previousNotificationPendingCount = globalPendingCount;
			return;
		}

		if (globalPendingCount > previousNotificationPendingCount && document.visibilityState === 'hidden') {
			const targetThreadId = inboxThreads[0]?.threadId ?? selectedThread?.id ?? null;
			void sendBrowserNotification(
				'EveryCode needs a response',
				globalPendingCount === 1
					? 'A new approval or question is waiting in the shared-session workspace.'
					: `${globalPendingCount} approvals or questions are now waiting in the shared-session workspace.`,
				'pending-actions',
				targetThreadId,
				'pending'
			);
		}

		previousNotificationPendingCount = globalPendingCount;
	});

	$effect(() => {
		if (!browser || !notificationsEnabled || notificationPermission !== 'granted') {
			seededCompletionNotifications = false;
			notifiedCompletionIds.clear();
			return;
		}

		const completionEvents = $workspaceState.events.filter((event) => event.method === 'turn/completed');
		if (!seededCompletionNotifications) {
			for (const event of completionEvents) {
				notifiedCompletionIds.add(event.id);
			}
			seededCompletionNotifications = true;
			return;
		}

		for (const event of completionEvents) {
			if (notifiedCompletionIds.has(event.id)) {
				continue;
			}

			notifiedCompletionIds.add(event.id);
			if (document.visibilityState !== 'hidden') {
				continue;
			}

			const thread = $workspaceState.threads.find((entry) => entry.id === event.threadId);
			const threadLabel = thread?.preview || event.threadId || 'Thread';
			void sendBrowserNotification(
				`${threadLabel} completed`,
				event.summary,
				`turn-completed:${event.id}`,
				event.threadId,
				'thread'
			);
		}
	});

	function syncDrafts() {
		domain.setWsUrl(wsUrl);
		domain.setStartCwd(startCwd);
		domain.setStartPrompt(startPrompt);
		domain.setComposerText(composerText);
	}

	async function connect() {
		if (connectBlockedByAcknowledgement) {
			showToastMessage('Confirm the single-user remote access boundary before connecting.');
			return;
		}

		syncDrafts();
		await domain.connect();
	}

	function toggleConnectionAcknowledgement(checked: boolean) {
		acknowledgedConnectionKey = checked ? (connectionAssessment.acknowledgementKey ?? '') : '';
	}

	async function refresh() {
		await domain.refreshThreads();
	}

	async function startThread() {
		syncDrafts();
		await domain.startThread();
	}

	async function sendPrompt() {
		syncDrafts();
		await domain.sendPrompt();
	}

	async function refreshForForeground() {
		await domain.refreshForForeground();
	}

	async function interruptSelectedTurn() {
		await domain.interruptSelectedTurn();
	}

	async function steerSelectedTurn() {
		const nextSteerText = steerText.trim();
		await domain.steerSelectedTurn(nextSteerText);
		steerText = '';
	}

	async function forkSelectedThread() {
		if (!selectedThread) {
			return;
		}

		const sourceThread = selectedThread;
		await domain.forkSelectedThread();
		const forkedThreadId = domain.getSnapshot().selectedThreadId;
		saveCheckpointRecord(sourceThread, forkedThreadId && forkedThreadId !== sourceThread.id ? forkedThreadId : null);
	}

	function saveCheckpointRecord(
		thread: ProtocolThread | null = selectedThread,
		forkedThreadId: string | null = null
	) {
		if (!thread) {
			return;
		}

		phase3Library = appendCheckpoint(phase3Library, {
			id: `checkpoint_${Date.now()}`,
			label: checkpointLabel.trim() || `Checkpoint ${new Date().toLocaleString()}`,
			note: checkpointNote.trim(),
			createdAt: Date.now(),
			sourceThreadId: thread.id,
			sourceThreadLabel: thread.preview || thread.id,
			forkedThreadId,
			cwd: thread.cwd,
			reviewArtifactCount: reviewArtifacts.length,
			reviewNoteCount: Object.values(selectedReviewNotes).filter((note) => note.trim()).length
		});
		checkpointLabel = '';
		checkpointNote = '';
		showToastMessage(forkedThreadId ? 'Checkpoint saved and branch created.' : 'Checkpoint saved.');
	}

	function draftExternalHandoff() {
		if (!selectedThread || !externalHandoffUrl.trim()) {
			return;
		}

		const url = externalHandoffUrl.trim();
		const source = detectExternalHandoffSource(url);
		const prompt = buildExternalHandoffPrompt(source, url, selectedThread.preview || selectedThread.id);
		domain.setComposerText(prompt);
		phase3Library = appendHandoff(phase3Library, {
			id: `handoff_${Date.now()}`,
			createdAt: Date.now(),
			source,
			url,
			title: `${source.toUpperCase()} handoff`,
			prompt,
			threadId: selectedThread.id
		});
		showToastMessage('External handoff drafted into the composer.');
	}

	async function exportReviewBundle() {
		if (!selectedThread || !browser) {
			return;
		}

		const payload = buildReviewExportPayload(
			selectedThread,
			reviewArtifacts,
			selectedReviewNotes,
			phase3Library.checkpoints
		);
		const text = JSON.stringify(payload, null, 2);
		await navigator.clipboard.writeText(text);
		phase3Library = appendReviewExport(phase3Library, {
			id: `export_${Date.now()}`,
			createdAt: Date.now(),
			threadId: selectedThread.id,
			threadLabel: selectedThread.preview || selectedThread.id,
			artifactCount: reviewArtifacts.length,
			reviewNoteCount: Object.values(selectedReviewNotes).filter((note) => note.trim()).length,
			byteLength: text.length
		});
		showToastMessage('Review bundle copied to the clipboard as JSON.');
	}

	async function openThread(threadId: string) {
		domain.selectThread(threadId);
		await domain.hydrateSelectedThread();
	}

	async function scrollToThreadRail() {
		await tickAfterDomWork();
		threadRailSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}

	async function scrollToCompanionTools() {
		companionToolsSection?.setAttribute('open', 'open');
		await tickAfterDomWork();
		companionToolsSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}

	async function focusRequestedThreadTarget() {
		await tickAfterDomWork();
		if (requestedFocus === 'pending') {
			pendingActionsSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
		} else if (requestedFocus === 'composer') {
			composerSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
		}

		if (requestedThreadId) {
			const url = new URL(window.location.href);
			url.searchParams.delete('thread');
			url.searchParams.delete('focus');
			window.history.replaceState({}, '', url);
			requestedThreadId = '';
		}
	}

	function toggleThreadPin(threadId: string) {
		threadNavigationState = togglePinnedThread(threadNavigationState, threadId);
	}

	async function resumeSelectedThread() {
		await domain.resumeSelectedThread();
	}

	async function handleTranscriptApprovalAction(cellId: string, action: string) {
		const request = selectedRequests.find(
			(entry): entry is SharedSessionApprovalRequest =>
				entry.kind !== 'question' && entry.id === cellId && entry.status === 'pending'
		);
		if (!request) {
			return;
		}

		const option = request.options.find((entry) => entry.label === action);
		if (!option) {
			return;
		}

		await domain.answerApproval(request.requestId, option.decision);
	}

	function setQuestionOption(questionId: string, selectedOption: string) {
		const current = questionDrafts[questionId] ?? { selectedOption: '', customValue: '' };
		questionDrafts = {
			...questionDrafts,
			[questionId]: { ...current, selectedOption }
		};
	}

	function setQuestionCustomValue(questionId: string, customValue: string) {
		const current = questionDrafts[questionId] ?? { selectedOption: '', customValue: '' };
		questionDrafts = {
			...questionDrafts,
			[questionId]: { ...current, customValue }
		};
	}

	function setQuestionDraftFromTranscript(cellId: string, selectedOption: string) {
		const question = selectedRequests.find(
			(entry): entry is SharedSessionQuestionRequest => entry.kind === 'question' && entry.id === cellId
		);
		if (!question) {
			return;
		}

		setQuestionOption(responderDraftKey(question), selectedOption);
	}

	function setQuestionCustomValueFromTranscript(cellId: string, customValue: string) {
		const question = selectedRequests.find(
			(entry): entry is SharedSessionQuestionRequest => entry.kind === 'question' && entry.id === cellId
		);
		if (!question) {
			return;
		}

		setQuestionCustomValue(responderDraftKey(question), customValue);
	}

	async function submitQuestionGroup(group: {
		requestId: SharedSessionQuestionRequest['requestId'];
		questions: SharedSessionQuestionRequest[];
	}) {
		const answers = Object.fromEntries(
			group.questions.map((question) => [question.questionId, questionAnswerList(question, questionDrafts)])
		);
		await domain.answerQuestions(group.requestId, answers);
		questionDrafts = clearResponderDrafts(
			questionDrafts,
			group.questions.map((question) => responderDraftKey(question))
		);
		focusedQuestionRequestId = null;
	}

	async function submitTranscriptQuestion(cellId: string) {
		const question = selectedRequests.find(
			(entry): entry is SharedSessionQuestionRequest => entry.kind === 'question' && entry.id === cellId
		);
		if (!question) {
			return;
		}

		const answers = questionAnswerList(question, questionDrafts);
		if (!answers.length) {
			return;
		}

		await domain.answerQuestions(question.requestId, { [question.questionId]: answers });
		questionDrafts = clearResponderDrafts(questionDrafts, [responderDraftKey(question)]);
	}

	async function handleSessionAction(action: 'connect' | 'resume' | 'take_over') {
		if (action === 'connect') {
			await connect();
			return;
		}

		if (action === 'take_over') {
			await domain.takeOverSelectedThread();
			return;
		}

		await resumeSelectedThread();
	}

	function focusQuestionResponder(cellId: string) {
		const question = selectedRequests.find(
			(entry): entry is SharedSessionQuestionRequest => entry.kind === 'question' && entry.id === cellId
		);
		focusedQuestionRequestId = question ? String(question.requestId) : null;
		responderSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}

	function threadTone(entry: ThreadNavigationEntry) {
		if (entry.isLive) {
			return 'border-amber-300/30 bg-amber-100/8 shadow-[0_12px_30px_rgba(0,0,0,0.16)]';
		}

		if (entry.isLoadedElsewhere) {
			return 'border-violet-300/24 bg-violet-200/8 shadow-[0_12px_30px_rgba(0,0,0,0.14)]';
		}

		if (entry.isSelected) {
			return 'border-sky-300/30 bg-sky-100/8 shadow-[0_12px_30px_rgba(0,0,0,0.14)]';
		}

		if (entry.isUnread) {
			return 'border-emerald-300/25 bg-emerald-200/8 shadow-[0_12px_30px_rgba(0,0,0,0.12)]';
		}

		return 'border-white/8 bg-black/14 hover:bg-white/6';
	}

	function sourceBadge(thread: ProtocolThread) {
		if (typeof thread.source === 'string') {
			return thread.source;
		}

		const subAgent = thread.source.subAgent;
		if (typeof subAgent === 'string') {
			return subAgent;
		}

		if ('thread_spawn' in subAgent) {
			return 'spawn';
		}

		if ('other' in subAgent) {
			return subAgent.other;
		}

		return 'unknown';
	}

	function looksLikeThreadId(value: string | null | undefined) {
		return Boolean(value?.match(/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i));
	}

	function threadDisplayLabel(thread: ProtocolThread | null | undefined) {
		if (!thread) {
			return 'No thread selected';
		}

		const preview = thread.preview?.trim();
		if (preview && !looksLikeThreadId(preview)) {
			return preview;
		}

		return thread.turns.length > 0 ? 'Untitled thread' : 'New thread';
	}

	function compactThreadPath(path: string | null | undefined) {
		if (!path) {
			return '';
		}

		const normalized = path.replace(/\\/g, '/');
		const parts = normalized.split('/').filter(Boolean);
		if (parts.length <= 2) {
			return path;
		}

		return `.../${parts.slice(-2).join('/')}`;
	}

	function threadContextLabel(thread: ProtocolThread) {
		const parts = [thread.gitInfo?.branch?.trim(), compactThreadPath(thread.cwd)].filter(Boolean);

		return parts.join(' · ');
	}

	function showThreadSourceBadge(thread: ProtocolThread) {
		return sourceBadge(thread) !== 'cli';
	}

	function formatUpdatedAt(timestamp: number) {
		return new Intl.DateTimeFormat('en-US', {
			month: 'short',
			day: 'numeric',
			hour: 'numeric',
			minute: '2-digit'
		}).format(timestamp * 1000);
	}

	function phaseTone() {
		switch ($workspaceState.connectionPhase) {
			case 'connected':
				return 'bg-emerald-500/15 text-emerald-100 ring-1 ring-emerald-400/30';
			case 'connecting':
				return 'bg-sky-500/15 text-sky-100 ring-1 ring-sky-400/30';
			case 'error':
				return 'bg-rose-500/15 text-rose-100 ring-1 ring-rose-400/30';
			default:
				return 'bg-white/8 text-stone-200 ring-1 ring-white/12';
		}
	}

	function connectionAssessmentTone(tone: ConnectionAssessmentTone) {
		switch (tone) {
			case 'positive':
				return 'bg-emerald-500/15 text-emerald-100 ring-1 ring-emerald-400/30';
			case 'warning':
				return 'bg-amber-500/15 text-amber-100 ring-1 ring-amber-400/30';
			case 'danger':
				return 'bg-rose-500/15 text-rose-100 ring-1 ring-rose-400/30';
			default:
				return 'bg-white/8 text-stone-200 ring-1 ring-white/12';
		}
	}

	function phaseLabel() {
		switch ($workspaceState.connectionPhase) {
			case 'connected':
				return 'Browser is live and ready.';
			case 'connecting':
				return 'Connecting this browser now.';
			case 'error':
				return 'Connection needs attention.';
			default:
				return 'Bring your local workspace into the browser.';
		}
	}

	function phaseHint() {
		switch ($workspaceState.connectionPhase) {
			case 'connected':
				return 'Threads, transcript updates, and approvals now stay in sync here.';
			case 'connecting':
				return 'Keep this tab open while the browser connects and your thread list loads.';
			case 'error':
				return 'Check the server address and reconnect. Your threads and pending actions will reappear when the connection is healthy.';
			default:
				return 'Connect once, then reopen a saved thread or start a new one without losing continuity.';
		}
	}

	function selectedPendingCount() {
		return selectedRequests.filter((request) => request.status === 'pending').length;
	}

	function browserTitle() {
		if (globalPendingCount > 0) {
			return `(${globalPendingCount}) EveryCode Shared Session Workspace`;
		}

		if (unreadThreadCount > 0) {
			return `(${unreadThreadCount}) EveryCode Shared Session Workspace`;
		}

		if (selectedActiveTurn) {
			return 'Live - EveryCode Shared Session Workspace';
		}

		return 'EveryCode Shared Session Workspace';
	}

	function currentSignalLabel() {
		if (selectedActiveTurn) {
			return 'Live turn';
		}

		if (selectedPendingCount() > 0) {
			return 'Needs response';
		}

		if ($workspaceState.pendingAction) {
			return 'Working';
		}

		return 'Idle';
	}

	function currentSignalTone() {
		if (selectedActiveTurn) {
			return 'bg-emerald-500/12 text-emerald-100 ring-1 ring-emerald-400/30';
		}

		if (selectedPendingCount() > 0) {
			return 'bg-amber-500/12 text-amber-100 ring-1 ring-amber-400/30';
		}

		if ($workspaceState.pendingAction) {
			return 'bg-sky-500/12 text-sky-100 ring-1 ring-sky-400/30';
		}

		return 'bg-white/8 text-stone-200 ring-1 ring-white/12';
	}

	async function runMobilePrimaryAction() {
		if (selectedActiveTurn) {
			await interruptSelectedTurn();
			return;
		}

		if (sessionBanner?.primaryAction) {
			await handleSessionAction(sessionBanner.primaryAction);
			return;
		}

		if (selectedTakeoverQueued) {
			pendingActionsSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
			return;
		}

		composerSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
	}

	async function openInboxThread(entry: PendingThreadInboxEntry) {
		if (!selectedThread || selectedThread.id !== entry.threadId) {
			await openThread(entry.threadId);
		}

		queueMicrotask(() => {
			pendingActionsSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
		});
	}

	async function runHeroPrimaryAction() {
		if ($workspaceState.connectionPhase !== 'connected') {
			showConnectionSettings = true;
			await connect();
			return;
		}

		if (sessionBanner?.primaryAction) {
			await handleSessionAction(sessionBanner.primaryAction);
			return;
		}

		if (selectedThread) {
			composerSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
			return;
		}

		await startThread();
	}

	function heroPrimaryLabel() {
		if ($workspaceState.connectionPhase !== 'connected') {
			return $workspaceState.connectionPhase === 'connecting' ? 'Connecting...' : 'Connect browser';
		}

		if (sessionBanner?.primaryLabel) {
			return sessionBanner.primaryLabel;
		}

		if (selectedThread) {
			return 'Continue this thread';
		}

		return 'Start a new thread';
	}

	function heroSecondaryLabel() {
		if (inboxThreads.length > 0) {
			return 'Review waiting threads';
		}

		if (selectedThread || $workspaceState.threads.length > 0) {
			return 'Browse saved threads';
		}

		return 'Companion tools';
	}

	async function runHeroSecondaryAction() {
		if (inboxThreads.length > 0) {
			await tickAfterDomWork();
			pendingActionsSection?.scrollIntoView({ behavior: 'smooth', block: 'start' });
			return;
		}

		if (selectedThread || $workspaceState.threads.length > 0) {
			await scrollToThreadRail();
			return;
		}

		await scrollToCompanionTools();
	}

	function inboxCardTone(entry: PendingThreadInboxEntry) {
		if (entry.isSelected) {
			return 'border-amber-300/28 bg-amber-200/10 shadow-[0_18px_40px_rgba(0,0,0,0.18)]';
		}

		if (entry.isLive) {
			return 'border-emerald-300/24 bg-emerald-300/8';
		}

		return 'border-white/10 bg-black/18';
	}

	function inboxActionLabel(entry: PendingThreadInboxEntry) {
		if (entry.isSelected) {
			return 'Jump to blocker';
		}

		return entry.isLive ? 'Open live thread' : 'Open and answer';
	}

	function activityFilterCount(filter: 'all' | 'selected' | 'blockers') {
		if (filter === 'all') {
			return $workspaceState.events.length;
		}

		if (filter === 'selected') {
			return $workspaceState.events.filter((event) => event.threadId === selectedThread?.id).length;
		}

		return $workspaceState.events.filter(
			(event) =>
				event.method.includes('requestApproval') ||
				event.method.includes('requestUserInput') ||
				event.method.includes('interrupted') ||
				event.method.includes('failed')
		).length;
	}

	function notificationsSupported() {
		return browser && 'Notification' in window;
	}

	function wakeLockSupported() {
		return browser && 'wakeLock' in navigator;
	}

	function voiceSupported() {
		return browser && supportsVoiceInput(window);
	}

	function notificationsStatusLabel() {
		if (!notificationsSupported()) {
			return 'This browser does not support notifications here.';
		}

		if (notificationPermission === 'granted' && notificationsEnabled) {
			return 'Blockers and completed turns can notify you while this tab is in the background.';
		}

		if (notificationPermission === 'denied') {
			return 'Browser notifications are blocked. Re-enable them in browser settings to restore blocker and completion alerts.';
		}

		return 'Enable notifications so the companion can surface blockers and completions while you are away from the tab.';
	}

	function installStatusLabel() {
		if (deferredInstallPrompt) {
			return installMessage || 'Install the companion for quicker relaunch and better mobile handoff.';
		}

		return installMessage || 'On iPhone and iPad, use Safari Share > Add to Home Screen once the companion is open.';
	}

	function mobileContinuityLabel() {
		if (!isOnline) {
			return 'This device is offline. Reconnect to keep the shared session fresh.';
		}

		if (pageVisibility === 'hidden') {
			return 'This tab is in the background. Alerts and reconnect behavior stay armed for away-from-desk monitoring.';
		}

		if (wakeLockStatus === 'active') {
			return 'The screen wake lock is active for this live turn.';
		}

		return 'Foreground sync, notification deep links, and optional wake lock keep mobile monitoring reliable.';
	}

	function wakeLockStatusLabel() {
		if (wakeLockStatus === 'unsupported') {
			return 'Wake lock unavailable';
		}

		if (wakeLockStatus === 'active') {
			return 'Screen awake';
		}

		if (wakeLockStatus === 'error') {
			return wakeLockErrorMessage || 'Wake lock failed';
		}

		return keepAwakeEnabled ? 'Wake lock armed' : 'Wake lock off';
	}

	async function requestNotifications() {
		if (!notificationsSupported()) {
			return;
		}

		notificationPermission = await Notification.requestPermission();
		notificationsEnabled = notificationPermission === 'granted';
	}

	function disableNotifications() {
		notificationsEnabled = false;
	}

	function toggleKeepAwake() {
		keepAwakeEnabled = !keepAwakeEnabled;
		if (!keepAwakeEnabled) {
			void releaseWakeLock();
		}
	}

	async function promptInstall() {
		if (!deferredInstallPrompt) {
			return;
		}

		await deferredInstallPrompt.prompt();
		const choice = await deferredInstallPrompt.userChoice;
		installMessage =
			choice.outcome === 'accepted'
				? 'Companion install accepted for this device.'
				: 'Install dismissed. You can trigger it again the next time the browser offers it.';
		if (choice.outcome === 'accepted') {
			deferredInstallPrompt = null;
		}
	}

	async function sendBrowserNotification(
		title: string,
		body: string,
		tag: string,
		threadId: string | null = null,
		focus: NotificationFocusTarget = 'thread'
	) {
		if (!notificationsSupported() || notificationPermission !== 'granted') {
			return;
		}

		const data = {
			url: buildNotificationUrl(threadId, focus)
		};
		const registration = 'serviceWorker' in navigator ? await navigator.serviceWorker.getRegistration() : null;
		if (registration) {
			await registration.showNotification(title, {
				body,
				tag,
				silent: false,
				icon: '/every-code-icon.svg',
				badge: '/every-code-icon.svg',
				data
			});
			return;
		}

		new Notification(title, {
			body,
			tag,
			silent: false,
			data
		});
	}

	function speechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
		if (!browser) {
			return null;
		}

		const voiceWindow = window as Window & {
			SpeechRecognition?: BrowserSpeechRecognitionConstructor;
			webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor;
		};

		return voiceWindow.SpeechRecognition ?? voiceWindow.webkitSpeechRecognition ?? null;
	}

	async function toggleVoiceCapture() {
		if (voiceCaptureState === 'listening') {
			stopVoiceCapture();
			return;
		}

		const Recognition = speechRecognitionConstructor();
		if (!Recognition) {
			voiceCaptureState = 'unsupported';
			return;
		}

		voiceErrorMessage = '';
		interimVoiceTranscript = '';
		voiceCaptureState = 'listening';

		const recognition = new Recognition();
		voiceRecognition = recognition;
		recognition.continuous = true;
		recognition.interimResults = true;
		recognition.lang = navigator.language || 'en-US';
		recognition.onresult = (event) => {
			let finalTranscript = '';
			let interimTranscript = '';

			for (let index = event.resultIndex; index < event.results.length; index += 1) {
				const result = event.results[index];
				const transcript = result?.[0]?.transcript ?? '';
				if (result?.isFinal) {
					finalTranscript += transcript;
				} else {
					interimTranscript += transcript;
				}
			}

			interimVoiceTranscript = interimTranscript.trim();
			if (!finalTranscript.trim()) {
				return;
			}

			const nextDraft = appendVoiceTranscript(composerText, finalTranscript);
			composerText = nextDraft;
			domain.setComposerText(nextDraft);
		};
		recognition.onerror = (event) => {
			voiceCaptureState = 'error';
			voiceErrorMessage = describeVoiceError(event.error, event.message);
			interimVoiceTranscript = '';
		};
		recognition.onend = () => {
			voiceRecognition = null;
			interimVoiceTranscript = '';
			if (voiceCaptureState === 'listening') {
				voiceCaptureState = 'idle';
			}
		};
		recognition.start();
	}

	function stopVoiceCapture() {
		voiceRecognition?.stop();
		voiceRecognition = null;
		interimVoiceTranscript = '';
		if (voiceCaptureState === 'listening') {
			voiceCaptureState = voiceSupported() ? 'idle' : 'unsupported';
		}
	}

	async function acquireWakeLock() {
		if (!wakeLockSupported() || wakeLockSentinel || pageVisibility !== 'visible') {
			return;
		}

		try {
			wakeLockSentinel = await (navigator as Navigator & {
				wakeLock: { request: (kind: 'screen') => Promise<BrowserWakeLockSentinel> };
			}).wakeLock.request('screen');
			wakeLockStatus = 'active';
			wakeLockErrorMessage = '';
		} catch (error) {
			wakeLockStatus = 'error';
			wakeLockErrorMessage = error instanceof Error ? error.message : 'Wake lock failed.';
		}
	}

	async function releaseWakeLock() {
		if (!wakeLockSentinel) {
			if (wakeLockSupported() && wakeLockStatus !== 'unsupported') {
				wakeLockStatus = keepAwakeEnabled ? 'idle' : 'idle';
			}
			return;
		}

		await wakeLockSentinel.release();
		wakeLockSentinel = null;
		if (wakeLockSupported()) {
			wakeLockStatus = keepAwakeEnabled ? 'idle' : 'idle';
		}
	}

	function buildNotificationUrl(threadId: string | null, focus: NotificationFocusTarget) {
		const url = new URL(window.location.origin);
		if (threadId) {
			url.searchParams.set('thread', threadId);
		}
		if (focus !== 'thread') {
			url.searchParams.set('focus', focus);
		}

		return url.toString();
	}

	function normalizeRequestedFocus(focus: string | null): NotificationFocusTarget {
		if (focus === 'pending' || focus === 'composer') {
			return focus;
		}

		return 'thread';
	}

	function describeVoiceError(error: string, message: string | undefined) {
		switch (error) {
			case 'not-allowed':
				return 'Microphone access was denied for voice capture.';
			case 'audio-capture':
				return 'No microphone was available for dictation.';
			case 'network':
				return 'Voice capture lost its network connection.';
			default:
				return message || `Voice capture failed: ${error}.`;
		}
	}

	function tickAfterDomWork() {
		return new Promise<void>((resolve) => {
			queueMicrotask(() => resolve());
		});
	}
</script>

<svelte:head>
	<title>{browserTitle()}</title>
	<meta name="description" content="Shared-session continuity workspace for EveryCode WebUI." />
	<link rel="preconnect" href="https://fonts.googleapis.com" />
	<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin="anonymous" />
	<link
		href="https://fonts.googleapis.com/css2?family=Fraunces:opsz,wght@9..144,500;9..144,600;9..144,700&family=IBM+Plex+Sans:wght@400;500;600&family=IBM+Plex+Mono:wght@400;500&display=swap"
		rel="stylesheet"
	/>
</svelte:head>

<div
	class="min-h-screen overflow-x-clip bg-[radial-gradient(circle_at_top,_rgba(43,84,87,0.42),_transparent_38%),linear-gradient(180deg,_#071012_0%,_#0d1718_45%,_#120f0d_100%)] text-stone-100"
>
	{#if showToast}
		<div class="pointer-events-none fixed top-4 right-4 z-40 max-w-sm rounded-[22px] border border-amber-300/25 bg-stone-950/92 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-100 shadow-[0_18px_40px_rgba(0,0,0,0.28)] backdrop-blur">
			{toastMessage}
		</div>
	{/if}
	<div class="mx-auto flex min-h-screen max-w-[1600px] flex-col px-4 py-4 lg:px-6 lg:py-6">
		<header
			class="mb-4 grid gap-4 rounded-[28px] border border-white/10 bg-[linear-gradient(135deg,_rgba(255,255,255,0.08),_rgba(255,255,255,0.03))] px-5 py-5 shadow-[0_20px_80px_rgba(0,0,0,0.35)] backdrop-blur md:grid-cols-[1.28fr_0.92fr] lg:mb-6 lg:px-7 lg:py-6"
		>
			<div class="space-y-4">
				<p
					class="text-[0.7rem] font-[var(--font-ui)] tracking-[0.28em] text-amber-200/72 uppercase"
				>
					EveryCode WebUI · Shared session workspace
				</p>
				<div class="space-y-3">
					<h1
						class="max-w-3xl text-[3.05rem] leading-[0.95] font-[var(--font-display)] tracking-[-0.04em] text-stone-50 sm:text-[3.35rem] md:text-[4rem] lg:text-[4.45rem]"
					>
						Pick up the same thread from the browser.
					</h1>
					<p
						class="max-w-2xl text-sm leading-7 font-[var(--font-ui)] text-stone-300 md:text-[15px]"
					>
						Open saved threads, keep the transcript live, and answer approvals or questions without
						losing the same shared EveryCode session.
					</p>
				</div>
				<div class="flex flex-wrap gap-3 text-xs font-[var(--font-ui)] text-stone-300/80">
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Real thread list</span
					>
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Live transcript</span
					>
					<span class="rounded-full border border-white/10 bg-white/5 px-3 py-1.5"
						>Answer approvals here</span
					>
				</div>
			</div>

			<section
				class="grid gap-3 rounded-[24px] border border-white/8 bg-black/18 p-4 shadow-inner shadow-black/20"
			>
				<div class="flex items-center justify-between gap-4">
					<div>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.24em] text-stone-400 uppercase"
						>
							Start here
						</p>
						<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">
							{phaseLabel()}
						</p>
					</div>
					<span class={`rounded-full px-3 py-1.5 text-xs font-[var(--font-ui)] ${phaseTone()}`}>
						{$workspaceState.connectionPhase}
					</span>
				</div>
				<p class="text-sm leading-6 font-[var(--font-ui)] text-stone-300">
					{phaseHint()}
				</p>
				<div class="rounded-[22px] border border-white/12 bg-white/7 px-4 py-4">
					<div class="flex flex-wrap items-start justify-between gap-3">
						<div>
							<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">What happens next</p>
							<p class="mt-2 text-sm leading-6 font-[var(--font-ui)] text-stone-100">
								{$workspaceState.statusMessage}
							</p>
						</div>
						<span class={`rounded-full px-3 py-1.5 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase ${currentSignalTone()}`}>
							{currentSignalLabel()}
						</span>
					</div>
					<div class="mt-4 flex flex-wrap gap-3 text-sm font-[var(--font-ui)]">
						<button
							type="button"
							onclick={runHeroPrimaryAction}
							class="rounded-full bg-teal-200 px-4 py-2.5 text-stone-950 transition hover:bg-teal-100"
						>
							{heroPrimaryLabel()}
						</button>
						<button
							type="button"
							onclick={runHeroSecondaryAction}
							class="rounded-full border border-white/12 bg-white/6 px-4 py-2.5 text-stone-100 transition hover:bg-white/10"
						>
							{heroSecondaryLabel()}
						</button>
						{#if $workspaceState.connectionPhase === 'connected'}
							<button
								type="button"
								onclick={refresh}
								class="rounded-full border border-white/12 bg-white/6 px-4 py-2.5 text-stone-100 transition hover:bg-white/10"
							>
								Refresh threads
							</button>
						{/if}
					</div>
				</div>
				<div class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-300 sm:grid-cols-2 xl:grid-cols-3">
					<div class="rounded-[22px] border border-white/12 bg-white/8 px-3 py-3">
						<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Saved threads</p>
						<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">{$workspaceState.threads.length}</p>
					</div>
					<div class="rounded-[22px] border border-white/12 bg-white/8 px-3 py-3">
						<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Waiting on you</p>
						<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">{globalPendingCount}</p>
					</div>
					<div class="rounded-[22px] border border-white/12 bg-white/8 px-3 py-3 sm:col-span-2 xl:col-span-1">
						<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Current thread</p>
						<p class="mt-2 text-sm leading-6 text-stone-100">
							{selectedThread ? threadDisplayLabel(selectedThread) : 'Not open yet'}
						</p>
					</div>
				</div>
			</section>
		</header>

		<details
			bind:this={companionToolsSection}
			class="mb-4 rounded-[24px] border border-white/10 bg-black/16 p-4 shadow-[0_16px_40px_rgba(0,0,0,0.18)] lg:mb-6"
		>
			<summary class="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
				<div>
					<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase">
						Companion tools
					</p>
					<p class="mt-2 text-sm leading-6 font-[var(--font-ui)] text-stone-300">
						Alerts, install flow, and away-from-desk continuity live here when you need them.
					</p>
				</div>
				<span class="rounded-full border border-white/10 bg-white/6 px-3 py-1.5 text-xs font-[var(--font-ui)] text-stone-200">
					Optional
				</span>
			</summary>
			<div class="mt-4 grid gap-3 text-sm font-[var(--font-ui)] text-stone-300 lg:grid-cols-3">
				<div class="rounded-[22px] border border-white/12 bg-white/8 px-4 py-4">
					<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Background alerts</p>
					<div class="mt-2 flex flex-wrap items-center justify-between gap-3">
						<p class="max-w-2xl text-sm leading-6 text-stone-100">{notificationsStatusLabel()}</p>
						<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
							{#if notificationPermission === 'granted' && notificationsEnabled}
								<button
									type="button"
									onclick={disableNotifications}
									class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12"
								>
									Disable alerts
								</button>
							{:else}
								<button
									type="button"
									onclick={requestNotifications}
									disabled={!notificationsSupported() || notificationPermission === 'denied'}
									class="rounded-full bg-teal-200 px-4 py-2 text-stone-950 transition hover:bg-teal-100 disabled:cursor-not-allowed disabled:opacity-45"
								>
									Enable alerts
								</button>
							{/if}
						</div>
					</div>
				</div>
				<div class="rounded-[22px] border border-white/12 bg-white/8 px-4 py-4">
					<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Install app</p>
					<div class="mt-2 flex flex-wrap items-center justify-between gap-3">
						<p class="max-w-2xl text-sm leading-6 text-stone-100">{installStatusLabel()}</p>
						<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
							<button
								type="button"
								onclick={promptInstall}
								disabled={!deferredInstallPrompt}
								class="rounded-full bg-amber-200 px-4 py-2 text-stone-950 transition hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-45"
							>
								Install app
							</button>
						</div>
					</div>
				</div>
				<div class="rounded-[22px] border border-white/12 bg-white/8 px-4 py-4">
					<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Away-from-desk continuity</p>
					<p class="mt-2 max-w-2xl text-sm leading-6 text-stone-100">{mobileContinuityLabel()}</p>
					<div class="mt-3 flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.16em] uppercase">
						<span class={`rounded-full border px-2.5 py-1 ${isOnline ? 'border-emerald-300/25 bg-emerald-300/10 text-emerald-100' : 'border-rose-300/25 bg-rose-300/10 text-rose-100'}`}>
							{isOnline ? 'network online' : 'network offline'}
						</span>
						<span class="rounded-full border border-white/12 bg-white/6 px-2.5 py-1 text-stone-100">
							{pageVisibility === 'visible' ? 'tab visible' : 'tab hidden'}
						</span>
						<span class={`rounded-full border px-2.5 py-1 ${wakeLockStatus === 'active' ? 'border-amber-300/25 bg-amber-300/10 text-amber-100' : 'border-white/12 bg-white/6 text-stone-100'}`}>
							{wakeLockStatusLabel()}
						</span>
						<span class={`rounded-full border px-2.5 py-1 ${voiceCaptureState === 'unsupported' ? 'border-white/12 bg-white/6 text-stone-100' : 'border-sky-300/25 bg-sky-300/10 text-sky-100'}`}>
							{voiceSupported() ? 'voice ready' : 'voice unavailable'}
						</span>
					</div>
					<div class="mt-4 flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
						<button
							type="button"
							onclick={refreshForForeground}
							disabled={$workspaceState.connectionPhase !== 'connected'}
							class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12 disabled:cursor-not-allowed disabled:opacity-45"
						>
							Sync now
						</button>
						<button
							type="button"
							onclick={toggleKeepAwake}
							disabled={!wakeLockSupported()}
							class="rounded-full bg-amber-200 px-4 py-2 text-stone-950 transition hover:bg-amber-100 disabled:cursor-not-allowed disabled:opacity-45"
						>
							{keepAwakeEnabled ? 'Disable stay awake' : 'Stay awake on live turn'}
						</button>
					</div>
				</div>
			</div>
		</details>

		{#if inboxThreads.length}
			<section class="mb-4 overflow-hidden rounded-[28px] border border-amber-300/14 bg-[linear-gradient(135deg,_rgba(38,28,17,0.94),_rgba(16,19,18,0.94))] shadow-[0_20px_80px_rgba(0,0,0,0.3)] lg:mb-6">
				<div class="grid gap-4 border-b border-white/8 px-5 py-5 md:grid-cols-[1.15fr_0.85fr] md:px-6">
					<div>
						<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.24em] text-amber-200/74 uppercase">
							Unblock inbox
						</p>
						<h2 class="mt-3 text-[2.2rem] leading-none font-[var(--font-display)] tracking-[-0.04em] text-stone-50">
							Threads waiting on you are collected here.
						</h2>
						<p class="mt-3 max-w-2xl text-sm leading-7 font-[var(--font-ui)] text-stone-300">
							Instead of hunting through the thread rail, open the next blocked thread directly and land near the approvals or grouped questions that still need a response.
						</p>
					</div>
					<div class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200 sm:grid-cols-3 md:grid-cols-1 xl:grid-cols-3">
						<div class="rounded-[22px] border border-white/10 bg-white/6 px-4 py-3">
							<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Blocked threads</p>
							<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">{inboxThreads.length}</p>
						</div>
						<div class="rounded-[22px] border border-white/10 bg-white/6 px-4 py-3">
							<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Approvals waiting</p>
							<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">{globalApprovalCount}</p>
						</div>
						<div class="rounded-[22px] border border-white/10 bg-white/6 px-4 py-3">
							<p class="text-[0.68rem] tracking-[0.18em] text-stone-400 uppercase">Question sets</p>
							<p class="mt-2 text-2xl font-[var(--font-display)] text-stone-50">{globalQuestionSetCount}</p>
						</div>
					</div>
				</div>

				<div class="grid gap-3 px-4 py-4 md:grid-cols-2 md:px-6 xl:grid-cols-3">
					{#each inboxThreads as entry}
						<section class={`rounded-[24px] border p-4 transition ${inboxCardTone(entry)}`}>
							<div class="flex items-start justify-between gap-3">
								<div>
									<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-400 uppercase">
										{entry.isSelected ? 'Open now' : entry.isLive ? 'Live elsewhere' : 'Needs response'}
									</p>
									<h3 class="mt-2 text-[1.45rem] leading-tight font-[var(--font-display)] text-stone-50">
										{entry.title}
									</h3>
								</div>
								<span class="rounded-full border border-white/10 bg-white/8 px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-200 uppercase">
									{entry.pendingActionCount} waiting
								</span>
							</div>
							<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-200">
								{entry.summary}
							</p>
							<p class="mt-3 line-clamp-2 text-xs leading-5 font-[var(--font-ui)] text-stone-400">
								{entry.cwd}
							</p>
							<div class="mt-4 flex flex-wrap gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase">
								{#if entry.isSelected}
									<span class="rounded-full border border-amber-300/24 bg-amber-300/10 px-2.5 py-1 text-amber-100">
										Selected thread
									</span>
								{/if}
								{#if entry.isLive}
									<span class="rounded-full border border-emerald-300/24 bg-emerald-300/10 px-2.5 py-1 text-emerald-100">
										Live updates attached
									</span>
								{/if}
								{#if entry.pendingApprovalCount > 0}
									<span class="rounded-full border border-white/10 bg-white/6 px-2.5 py-1 text-stone-200">
										{entry.pendingApprovalCount} approval{entry.pendingApprovalCount === 1 ? '' : 's'}
									</span>
								{/if}
								{#if entry.pendingQuestionGroupCount > 0}
									<span class="rounded-full border border-white/10 bg-white/6 px-2.5 py-1 text-stone-200">
										{entry.pendingQuestionGroupCount} question set{entry.pendingQuestionGroupCount === 1 ? '' : 's'}
									</span>
								{/if}
							</div>
							<button
								type="button"
								onclick={() => openInboxThread(entry)}
								class="mt-4 rounded-[20px] bg-amber-300 px-4 py-3 text-sm font-[var(--font-ui)] font-semibold text-amber-950 transition hover:bg-amber-200"
							>
								{inboxActionLabel(entry)}
							</button>
						</section>
					{/each}
				</div>
			</section>
		{/if}

		<div class="grid min-h-0 flex-1 gap-4 lg:grid-cols-[320px_minmax(0,1fr)]">
			<aside
				bind:this={threadRailSection}
				class="overflow-hidden rounded-[28px] border border-white/10 bg-[linear-gradient(180deg,_rgba(255,255,255,0.07),_rgba(255,255,255,0.03))] p-4 shadow-[0_16px_60px_rgba(0,0,0,0.25)] backdrop-blur lg:sticky lg:top-6 lg:self-start"
			>
				<div class="mb-4 flex items-center justify-between gap-3">
					<div>
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
						>
							Saved threads
						</p>
						<h2 class="mt-2 text-3xl font-[var(--font-display)] text-stone-50">Your threads</h2>
					</div>
					<button
						type="button"
						onclick={refresh}
						class="rounded-full border border-white/10 bg-white/5 px-3 py-2 text-xs font-[var(--font-ui)] text-stone-200 transition hover:bg-white/10"
						>Reload</button
					>
				</div>

				<label class="mb-4 grid gap-2 text-sm font-[var(--font-ui)] text-stone-200">
					<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase">Find a thread</span>
					<input
						type="search"
						bind:value={threadSearch}
						class="rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
						placeholder="Search by thread, branch, path, or id"
					/>
				</label>

				<div class="space-y-3 rounded-[24px] border border-white/8 bg-black/14 p-4">
					<div class="flex items-start justify-between gap-3">
						<div>
							<p
								class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-400 uppercase"
							>
							{$workspaceState.connectionPhase === 'connected' && !showConnectionSettings
								? 'Connected'
								: 'Connection'}
							</p>
							<p class="mt-2 max-w-[13rem] text-xs leading-5 font-[var(--font-ui)] text-stone-400">
								{$workspaceState.connectionPhase === 'connected' && !showConnectionSettings
									? 'The browser is already synced. Reopen these settings only when you need a different app-server.'
									: 'Save the server address here, then return whenever you want the same threads in the browser.'}
							</p>
						</div>
						<button
							type="button"
							onclick={() => (showConnectionSettings = !showConnectionSettings)}
							class="rounded-full border border-white/10 bg-white/5 px-3 py-2 text-[11px] font-[var(--font-ui)] text-stone-200 transition hover:bg-white/10"
						>
							{showConnectionSettings ? 'Hide settings' : 'Edit address'}
						</button>
					</div>
					<div class="flex flex-wrap items-center gap-2 text-xs font-[var(--font-ui)] text-stone-300">
						<span class={`rounded-full px-2.5 py-1 ${phaseTone()}`}>
							{$workspaceState.connectionPhase}
						</span>
						{#if $workspaceState.connectionPhase === 'connected' && !showConnectionSettings}
							<span class="min-w-0 truncate font-[var(--font-mono)] text-[11px] text-stone-400" title={wsUrl}>
								{wsUrl}
							</span>
						{:else}
							<span>
								{$workspaceState.connectionPhase === 'connected'
									? 'This browser can open, resume, and answer threads.'
									: 'Connect to load your saved threads here.'}
							</span>
						{/if}
					</div>
					{#if persistentRemoteBoundaryNotice && !(showConnectionSettings || $workspaceState.connectionPhase !== 'connected')}
						<p class="rounded-2xl border border-amber-300/16 bg-amber-200/8 px-3 py-3 text-[11px] leading-5 font-[var(--font-ui)] text-amber-100/88">
							{persistentRemoteBoundaryNotice}
						</p>
					{/if}
					{#if showConnectionSettings || $workspaceState.connectionPhase !== 'connected'}
						<label class="grid gap-2 text-sm font-[var(--font-ui)] text-stone-200">
							<span class="text-[0.68rem] tracking-[0.2em] text-stone-400 uppercase"
								>Server address</span
							>
							<input
								type="text"
								bind:value={wsUrl}
								oninput={() => domain.setWsUrl(wsUrl)}
								class="rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
								placeholder="ws://127.0.0.1:8877"
							/>
						</label>
						<div class="rounded-[22px] border border-white/10 bg-white/6 p-4">
							<div class="flex items-start justify-between gap-3">
								<div>
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-400 uppercase"
									>
								How this browser connects
									</p>
									<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-100">
										{connectionAssessment.headline}
									</p>
								</div>
								<span
									class={`rounded-full px-2.5 py-1 text-[11px] font-[var(--font-ui)] ${connectionAssessmentTone(connectionAssessment.tone)}`}
								>
									{connectionAssessment.badge}
								</span>
							</div>
							<p class="mt-3 text-xs leading-5 font-[var(--font-ui)] text-stone-300">
								{connectionAssessment.detail}
							</p>
							<p class="mt-2 text-xs leading-5 font-[var(--font-ui)] text-stone-400">
								{connectionAssessment.recommendation}
							</p>
							<div class="mt-4 rounded-2xl border border-white/8 bg-black/20 p-3">
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-500 uppercase"
								>
									Supported setup
								</p>
								<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-100">
									{connectionAssessment.guide.title}
								</p>
								<p class="mt-2 text-xs leading-5 font-[var(--font-ui)] text-stone-300">
									{connectionAssessment.guide.summary}
								</p>
								<p class="mt-2 rounded-2xl border border-amber-300/18 bg-amber-200/8 px-3 py-2 text-[11px] leading-5 font-[var(--font-ui)] text-amber-100/88">
									{connectionAssessment.guide.identityBoundary}
								</p>
								<div class="mt-3 space-y-3">
									{#each connectionAssessment.guide.steps as step}
										<div class="rounded-2xl border border-white/6 bg-white/4 px-3 py-2">
											<p class="text-[11px] font-[var(--font-ui)] text-stone-100">
												{step.label}
											</p>
											<p class="mt-1 text-[11px] leading-5 font-[var(--font-ui)] text-stone-400">
												{step.detail}
											</p>
										</div>
									{/each}
								</div>
								<p class="mt-3 text-[11px] leading-5 font-[var(--font-ui)] text-stone-500">
									More setup notes are available in the repo docs if you need them.
								</p>
							</div>
							{#if connectionAssessment.requiresAcknowledgement && connectionAssessment.acknowledgementLabel}
								<label class="mt-3 flex items-start gap-3 rounded-2xl border border-amber-300/16 bg-amber-200/8 px-3 py-3 text-[11px] leading-5 font-[var(--font-ui)] text-amber-50">
									<input
										type="checkbox"
										checked={connectionAcknowledged}
										onchange={(event) =>
											toggleConnectionAcknowledgement(
												(event.currentTarget as HTMLInputElement).checked
											)}
										class="mt-0.5 h-4 w-4 rounded border-white/20 bg-stone-950/80 text-amber-300 focus:ring-amber-300/30"
									/>
									<span>{connectionAssessment.acknowledgementLabel}</span>
								</label>
							{/if}
							{#if connectionAssessment.guide.codeSample}
								<div class="mt-3 rounded-2xl border border-white/8 bg-black/20 p-3">
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-500 uppercase"
									>
										{connectionAssessment.guide.codeSampleLabel}
									</p>
									<code class="mt-2 block overflow-x-auto text-[11px] leading-5 text-stone-200"
										>{connectionAssessment.guide.codeSample}</code
									>
								</div>
							{/if}
						</div>
					{:else}
						<p class="rounded-2xl border border-white/10 bg-white/6 px-3 py-3 text-xs leading-5 font-[var(--font-ui)] text-stone-300">
							This saved address is working. Threads, transcript updates, and approvals will keep syncing here until you disconnect.
						</p>
					{/if}
					<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
						{#if showConnectionSettings || $workspaceState.connectionPhase !== 'connected'}
							<button
								type="button"
								onclick={connect}
								disabled={!connectionAssessment.canConnect || connectBlockedByAcknowledgement}
								class="rounded-full bg-teal-200 px-4 py-2 text-stone-950 transition hover:bg-teal-100 disabled:cursor-not-allowed disabled:bg-stone-500 disabled:text-stone-300"
								>{!connectionAssessment.canConnect
									? 'Fix address first'
									: connectBlockedByAcknowledgement
										? 'Confirm before connect'
										: 'Connect'}</button
							>
						{/if}
						<button
							type="button"
							onclick={() => domain.disconnect('Disconnected.')}
							class="rounded-full border border-white/12 bg-white/8 px-4 py-2 text-stone-100 transition hover:bg-white/12"
							>Disconnect</button
						>
					</div>
				</div>

				{#if $workspaceState.connectionPhase === 'connected' && $workspaceState.threads.length > 0}
					<details class="mt-4 rounded-[24px] border border-white/8 bg-black/14 p-4">
						<summary class="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
							<div>
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase">
									Fresh thread
								</p>
								<p class="mt-2 text-xs leading-5 font-[var(--font-ui)] text-stone-400">
									Open this only when you want a clean thread instead of resuming one from the rail.
								</p>
							</div>
							<span class="rounded-full border border-white/10 bg-white/6 px-3 py-1.5 text-xs font-[var(--font-ui)] text-stone-200">
								Optional
							</span>
						</summary>
						<div class="mt-4 space-y-3">
							<input
								type="text"
								bind:value={startCwd}
								oninput={() => domain.setStartCwd(startCwd)}
								class="w-full rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
								placeholder="/Users/cbusillo/Developer/code"
							/>
							<textarea
								rows="3"
								bind:value={startPrompt}
								oninput={() => domain.setStartPrompt(startPrompt)}
								class="w-full rounded-[24px] border border-white/12 bg-white/8 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
								placeholder="Optional first prompt..."
							></textarea>
							<button
								type="button"
								onclick={startThread}
								class="w-full rounded-[22px] bg-amber-200 px-4 py-3 text-sm font-[var(--font-ui)] font-medium text-stone-950 transition hover:bg-amber-100"
							>
								Start new thread
							</button>
						</div>
					</details>
				{:else}
					<div class="mt-4 space-y-3 rounded-[24px] border border-white/8 bg-black/14 p-4">
						<p
							class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
						>
							Start here
						</p>
						<p class="text-xs leading-5 font-[var(--font-ui)] text-stone-400">
							Launch a new thread from this browser, with an optional opening prompt.
						</p>
						<input
							type="text"
							bind:value={startCwd}
							oninput={() => domain.setStartCwd(startCwd)}
							class="w-full rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
							placeholder="/Users/cbusillo/Developer/code"
						/>
						<textarea
							rows="3"
							bind:value={startPrompt}
							oninput={() => domain.setStartPrompt(startPrompt)}
							class="w-full rounded-[24px] border border-white/12 bg-white/8 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
							placeholder="Optional first prompt..."
						></textarea>
						<button
							type="button"
							onclick={startThread}
							class="w-full rounded-[22px] bg-amber-200 px-4 py-3 text-sm font-[var(--font-ui)] font-medium text-stone-950 transition hover:bg-amber-100"
						>
							Start new thread
						</button>
					</div>
				{/if}

				<div class="mt-4 space-y-3">
					{#if !$workspaceState.threads.length}
						<div
							class="rounded-[24px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm leading-6 font-[var(--font-ui)] text-stone-400"
						>
							Connect and reload to see your saved threads.
						</div>
					{:else if !filteredThreads.length}
						<div
							class="rounded-[24px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm leading-6 font-[var(--font-ui)] text-stone-400"
						>
							No threads match that search yet.
						</div>
					{:else}
						{#if recentThreads.length}
							<section class="rounded-[24px] border border-white/8 bg-black/14 p-4">
								<div class="flex items-center justify-between gap-3">
									<div>
										<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.2em] text-stone-400 uppercase">
											Recent threads
										</p>
										<p class="mt-2 text-xs leading-5 font-[var(--font-ui)] text-stone-400">
											Return to the threads you opened most recently, even if newer idle sessions exist lower in the rail.
										</p>
									</div>
									<span class="rounded-full border border-white/10 bg-white/6 px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] text-stone-200 uppercase">
										{recentThreads.length} recent
									</span>
								</div>
								<div class="mt-3 flex snap-x gap-2 overflow-x-auto pb-1">
									{#each recentThreads as entry}
										<button
											type="button"
											onclick={() => openThread(entry.threadId)}
											class="min-w-[180px] shrink-0 snap-start rounded-[20px] border border-white/10 bg-white/7 px-3 py-3 text-left text-sm font-[var(--font-ui)] text-stone-100 transition hover:bg-white/10"
										>
											<p class="line-clamp-2 text-sm leading-5">{threadDisplayLabel(entry.thread)}</p>
											<p class="mt-2 text-[11px] tracking-[0.16em] text-stone-400 uppercase">
												{formatUpdatedAt(entry.thread.updatedAt)}
											</p>
										</button>
									{/each}
								</div>
							</section>
						{/if}
						<div class="flex snap-x gap-3 overflow-x-auto pb-1 lg:block lg:space-y-3 lg:overflow-visible lg:pb-0">
						{#each filteredThreads as entry}
							<section
								class={`w-[calc(100vw-3.5rem)] min-w-[280px] shrink-0 snap-start rounded-[24px] border px-4 py-4 transition sm:w-[340px] lg:w-full lg:min-w-0 ${threadTone(entry)}`}
							>
								<div class="mb-3 flex items-center justify-between gap-3">
									<div class="flex flex-wrap items-center gap-2 text-[11px] font-[var(--font-ui)] tracking-[0.16em] uppercase">
										{#if showThreadSourceBadge(entry.thread)}
											<span class="rounded-full border border-white/12 bg-white/8 px-2.5 py-1 text-stone-200">
												{sourceBadge(entry.thread)}
											</span>
										{/if}
										{#if entry.pendingCount}
											<span class="rounded-full bg-amber-400/14 px-2 py-0.5 text-amber-100 ring-1 ring-amber-300/25">
												{entry.pendingCount} waiting
											</span>
										{/if}
										{#if entry.isLive}
											<span class="rounded-full bg-emerald-400/14 px-2 py-0.5 text-emerald-100 ring-1 ring-emerald-300/25">
												Live
											</span>
										{/if}
										{#if entry.isLoadedElsewhere}
											<span class="rounded-full bg-violet-400/14 px-2 py-0.5 text-violet-100 ring-1 ring-violet-300/25">
												Open elsewhere
											</span>
										{/if}
										{#if entry.isUnread}
											<span class="inline-flex items-center gap-1.5 text-emerald-100">
												<span class="h-2 w-2 rounded-full bg-emerald-300"></span>
												New
											</span>
										{/if}
									</div>
									<div class="flex items-center gap-2">
										<button
											type="button"
											onclick={() => toggleThreadPin(entry.threadId)}
											class={`rounded-full px-2.5 py-1 text-[11px] font-[var(--font-ui)] tracking-[0.18em] uppercase transition ${entry.isPinned ? 'bg-amber-300 text-amber-950' : 'border border-white/12 bg-white/8 text-stone-200 hover:bg-white/12'}`}
										>
											{entry.isPinned ? 'Pinned' : 'Pin'}
										</button>
									</div>
								</div>
								<button
									type="button"
									onclick={() => openThread(entry.threadId)}
									class="block w-full text-left"
								>
									<div class="flex items-center justify-between gap-3">
										<h3 class="text-[1.35rem] leading-tight font-[var(--font-display)] text-stone-50">
											{threadDisplayLabel(entry.thread)}
										</h3>
										<span class="text-[11px] font-[var(--font-ui)] text-stone-400">
											{formatUpdatedAt(entry.thread.updatedAt)}
										</span>
									</div>
									<p
										class="mt-3 truncate text-xs font-[var(--font-ui)] text-stone-400"
										title={entry.thread.cwd}
									>
										{threadContextLabel(entry.thread) || compactThreadPath(entry.thread.cwd)}
									</p>
									</button>
								</section>
						{/each}
						</div>
					{/if}
				</div>
			</aside>

			<main class="min-w-0 space-y-4">
				{#if selectedThread}
					{#if pendingApprovals.length || groupedQuestionBatchesList.length}
						<section
							bind:this={pendingActionsSection}
							class="rounded-[28px] border border-white/10 bg-[linear-gradient(180deg,_rgba(255,255,255,0.08),_rgba(255,255,255,0.03))] p-4 shadow-[0_16px_60px_rgba(0,0,0,0.22)] backdrop-blur md:p-6"
						>
							<div class="mb-4 flex flex-wrap items-end justify-between gap-3">
								<div>
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
									>
										Pending actions
									</p>
									<h2 class="mt-2 text-3xl font-[var(--font-display)] text-stone-50">
										Answer what still needs you.
									</h2>
								</div>
								<p class="max-w-xl text-sm leading-6 font-[var(--font-ui)] text-stone-300">
									Inline approval cards stay in the transcript. This tray only collects grouped
									questions so you can answer them in one pass.
								</p>
							</div>

							<div class="grid gap-4 xl:grid-cols-[1fr_1fr]">
								<div class="space-y-4">
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase"
									>
										Approvals
									</p>
									{#if !pendingApprovals.length}
										<div
											class="rounded-[22px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm font-[var(--font-ui)] text-stone-400"
										>
											No pending approvals on this thread.
										</div>
									{:else}
										<div class="rounded-[24px] border border-white/10 bg-black/18 p-4 text-sm leading-6 font-[var(--font-ui)] text-stone-300">
											<p class="text-stone-50">{pendingApprovals.length} approval{pendingApprovals.length === 1 ? '' : 's'} pending.</p>
											<p class="mt-2">
												Approve these directly in the transcript so the active turn stays easy to read.
											</p>
										</div>
									{/if}
								</div>

								<div bind:this={responderSection} class="space-y-4">
									<p
										class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase"
									>
										Questions
									</p>
									{#if !groupedQuestionBatchesList.length}
										<div
											class="rounded-[22px] border border-dashed border-white/10 bg-white/4 px-4 py-5 text-sm font-[var(--font-ui)] text-stone-400"
										>
											No grouped question sets need the side tray right now.
										</div>
									{:else}
										{#each groupedQuestionBatchesList as group}
											<section class={`rounded-[24px] border bg-black/18 p-4 ${focusedQuestionRequestId === String(group.requestId) ? 'border-amber-300/50 shadow-[0_0_0_1px_rgba(252,211,77,0.2)]' : 'border-white/10'}`}>
												<p
													class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase"
												>
													{group.header || 'Question group'}
												</p>
												<div class="mt-4 space-y-4">
													{#each group.questions as question}
														{@const draftKey = responderDraftKey(question)}
														<div class="rounded-[20px] border border-white/8 bg-white/6 p-4">
															<p class="text-sm leading-6 font-[var(--font-ui)] text-stone-100">
																{question.prompt}
															</p>
															{#if question.options.length}
																<div class="mt-3 flex flex-wrap gap-2">
																	{#each question.options as option}
																		<button
																			type="button"
																			onclick={() => setQuestionOption(draftKey, option.label)}
																			class={`rounded-full px-3 py-2 text-xs font-[var(--font-ui)] transition ${questionDrafts[draftKey]?.selectedOption === option.label ? 'bg-stone-100 text-stone-950' : 'bg-white/8 text-stone-200 hover:bg-white/12'}`}
																		>
																			{option.label}
																		</button>
																	{/each}
																	{#if question.allowOther}
																		<button
																			type="button"
																			onclick={() => setQuestionOption(draftKey, '__other')}
																			class={`rounded-full px-3 py-2 text-xs font-[var(--font-ui)] transition ${questionDrafts[draftKey]?.selectedOption === '__other' ? 'bg-amber-200 text-amber-950' : 'bg-white/8 text-stone-200 hover:bg-white/12'}`}
																		>
																			Other
																		</button>
																	{/if}
																</div>
															{/if}
															{#if question.allowOther || question.allowSecret || !question.options.length}
																<input
																	type={question.allowSecret ? 'password' : 'text'}
																	value={questionDrafts[draftKey]?.customValue ?? ''}
																	oninput={(event) =>
																		setQuestionCustomValue(
																			draftKey,
																			(event.currentTarget as HTMLInputElement).value
																		)}
																	class="mt-3 w-full rounded-2xl border border-white/12 bg-white/8 px-4 py-3 text-sm text-stone-100 placeholder:text-stone-500 focus:border-teal-300/40 focus:ring-0"
																	placeholder={question.allowSecret
																		? 'Enter secret answer'
																		: question.allowOther
																			? 'Enter another answer'
																			: 'Enter your answer'}
																/>
															{/if}
														</div>
													{/each}
												</div>
												<button
													type="button"
													onclick={() => submitQuestionGroup(group)}
													disabled={!canSubmitQuestionGroup(group, questionDrafts)}
													class="mt-4 rounded-[20px] bg-teal-200 px-4 py-3 text-sm font-[var(--font-ui)] font-medium text-stone-950 transition hover:bg-teal-100 disabled:cursor-not-allowed disabled:opacity-45"
													>Send grouped answers</button
												>
											</section>
										{/each}
									{/if}
								</div>
							</div>
						</section>
					{/if}

					<TranscriptThreadView
						threadId={selectedThread.id}
						title={threadDisplayLabel(selectedThread)}
						description="Keep the transcript, blockers, and next prompt in one place."
						badges={transcriptBadges}
						cells={transcriptCells}
						sessionBanner={sessionBanner}
						canApprove={$workspaceState.connectionPhase === 'connected'}
						questionDraftsById={questionDraftsById}
						onSessionAction={handleSessionAction}
						onApprovalAction={handleTranscriptApprovalAction}
						onQuestionOptionSelect={setQuestionDraftFromTranscript}
						onQuestionCustomValue={setQuestionCustomValueFromTranscript}
						onQuestionSubmit={submitTranscriptQuestion}
						onQuestionFocus={focusQuestionResponder}
						emptyMessage="This thread is loaded, but no turn items are present yet. Resume it or send the next prompt to populate the transcript."
					/>

					<section
						bind:this={composerSection}
						class="sticky bottom-3 z-20 rounded-[28px] border border-white/10 bg-stone-950/96 px-4 py-4 text-stone-50 shadow-[0_14px_30px_rgba(0,0,0,0.18)] backdrop-blur md:static md:bg-stone-950 md:px-6"
						style="padding-bottom: calc(env(safe-area-inset-bottom, 0px) + 1rem);"
					>
						<div class="flex flex-wrap items-start justify-between gap-3">
							<div>
								<p
									class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase"
								>
									Next prompt
								</p>
								<p class="mt-2 max-w-2xl text-sm leading-6 font-[var(--font-ui)] text-stone-300">
									Send the next prompt from this browser. If the thread is not already active,
									EveryCode will attach it here first.
								</p>
							</div>
							<div class="flex flex-wrap gap-2 text-sm font-[var(--font-ui)]">
								<button
									type="button"
									onclick={toggleVoiceCapture}
									disabled={!voiceSupported()}
									class={`rounded-full px-4 py-2 transition ${voiceCaptureState === 'listening' ? 'bg-rose-300 text-rose-950 hover:bg-rose-200' : 'bg-teal-200 text-stone-950 hover:bg-teal-100'} disabled:cursor-not-allowed disabled:opacity-45`}
								>
									{voiceCaptureState === 'listening' ? 'Stop dictation' : 'Start dictation'}
								</button>
							</div>
						</div>
						<p class="mt-3 text-xs leading-6 font-[var(--font-ui)] text-stone-300">
							{voiceCaptureStatusLabel(voiceCaptureState, interimVoiceTranscript, voiceErrorMessage)}
						</p>
						<details class="mt-3 rounded-[22px] border border-white/10 bg-white/[0.04] px-4 py-4">
							<summary class="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
								<div>
									<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
										Thread tools
									</p>
									<p class="mt-2 max-w-2xl text-xs leading-6 font-[var(--font-ui)] text-stone-300">
										Open this when you need next-turn overrides, steer notes, checkpoints, or a handoff helper.
									</p>
								</div>
								<span class="rounded-full border border-white/12 bg-white/6 px-3 py-2 text-xs font-[var(--font-ui)] text-stone-100">
									Advanced
								</span>
							</summary>
							<div class="mt-4 rounded-[22px] border border-white/10 bg-black/18 px-4 py-4">
							<div class="flex flex-wrap items-start justify-between gap-3">
								<div>
									<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
									Next-turn options
									</p>
									<p class="mt-2 max-w-2xl text-xs leading-6 font-[var(--font-ui)] text-stone-300">
										These overrides apply to the next prompt you send here and stay with the thread until you change or clear them.
									</p>
								</div>
								<button
									type="button"
									onclick={() => {
										domain.setTurnOverrideModel('');
										domain.setTurnOverrideCwd('');
									}}
									class="rounded-full border border-white/12 bg-white/6 px-3 py-2 text-xs font-[var(--font-ui)] text-stone-100 transition hover:bg-white/10"
								>
									Clear overrides
								</button>
							</div>
							<div class="mt-4 grid gap-3 md:grid-cols-2">
								<label class="grid gap-2 text-xs font-[var(--font-ui)] text-stone-300">
									<span>Model override</span>
									<input
										type="text"
										bind:value={turnOverrideModel}
										oninput={() => domain.setTurnOverrideModel(turnOverrideModel)}
										class="rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
										placeholder="Use the thread default"
									/>
								</label>
								<label class="grid gap-2 text-xs font-[var(--font-ui)] text-stone-300">
									<span>Working directory override</span>
									<input
										type="text"
										bind:value={turnOverrideCwd}
										oninput={() => domain.setTurnOverrideCwd(turnOverrideCwd)}
										class="rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
										placeholder="Stay in the thread workspace"
									/>
								</label>
								<label class="grid gap-2 text-xs font-[var(--font-ui)] text-stone-300 md:col-span-2">
									<span>Personality override</span>
									<select
										bind:value={turnOverridePersonality}
										onchange={() => domain.setTurnOverridePersonality(turnOverridePersonality)}
										class="rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm text-stone-50 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									>
										<option value="">Use the thread default</option>
										<option value="pragmatic">Pragmatic</option>
										<option value="friendly">Friendly</option>
										<option value="none">None</option>
									</select>
								</label>
							</div>
						</div>
						<div class="mt-3 grid gap-3 xl:grid-cols-3">
							<section class="rounded-[22px] border border-white/10 bg-white/[0.04] px-4 py-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
									Guide the active turn
								</p>
								<p class="mt-2 text-xs leading-6 font-[var(--font-ui)] text-stone-300">
									Send a short follow-up instruction into the active turn without starting a brand new prompt.
								</p>
								<textarea
									bind:value={steerText}
									class="mt-3 min-h-[96px] rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm leading-6 text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									placeholder="While the turn is running, prioritize the diff summary and stop before any destructive change."
								></textarea>
								<button
									type="button"
									onclick={steerSelectedTurn}
									disabled={!selectedActiveTurn || !$workspaceState.connectionPhase || !$workspaceState.selectedThreadId}
									class="mt-3 rounded-[18px] border border-sky-300/25 bg-sky-200/10 px-4 py-3 text-sm font-[var(--font-ui)] text-sky-100 transition hover:bg-sky-200/16 disabled:cursor-not-allowed disabled:opacity-45"
								>
									Queue steer note
								</button>
							</section>
							<section class="rounded-[22px] border border-white/10 bg-white/[0.04] px-4 py-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
									Save a checkpoint
								</p>
								<p class="mt-2 text-xs leading-6 font-[var(--font-ui)] text-stone-300">
									Save a durable note for this thread, or fork it into a fresh thread you can reopen later.
								</p>
								<input
									type="text"
									bind:value={checkpointLabel}
									class="mt-3 w-full rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									placeholder="Checkpoint label"
								/>
								<textarea
									bind:value={checkpointNote}
									class="mt-3 min-h-[96px] rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm leading-6 text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									placeholder="What matters at this checkpoint: approvals left, review notes worth carrying forward, or why this branch exists."
								></textarea>
								<div class="mt-3 grid gap-2 sm:grid-cols-2">
									<button
										type="button"
										onclick={() => saveCheckpointRecord()}
										disabled={!selectedThread}
										class="rounded-[18px] border border-white/12 bg-white/6 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-45"
									>
										Save checkpoint
									</button>
									<button
										type="button"
										onclick={forkSelectedThread}
										disabled={!selectedThread}
										class="rounded-[18px] bg-emerald-300 px-4 py-3 text-sm font-[var(--font-ui)] font-semibold text-emerald-950 transition hover:bg-emerald-200 disabled:cursor-not-allowed disabled:opacity-45"
									>
										Fork thread
									</button>
								</div>
							</section>
							<section class="rounded-[22px] border border-white/10 bg-white/[0.04] px-4 py-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
									Share context elsewhere
								</p>
								<p class="mt-2 text-xs leading-6 font-[var(--font-ui)] text-stone-300">
									Draft the next prompt from a GitHub, Slack, Jira, or Linear update while keeping the thread context intact.
								</p>
								<input
									type="url"
									bind:value={externalHandoffUrl}
									class="mt-3 w-full rounded-[18px] border border-white/10 bg-stone-950/70 px-4 py-3 text-sm text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									placeholder="https://github.com/owner/repo/issues/123"
								/>
								<div class="mt-3 grid gap-2 sm:grid-cols-2">
									<button
										type="button"
										onclick={draftExternalHandoff}
										disabled={!selectedThread || !externalHandoffUrl.trim()}
										class="rounded-[18px] border border-amber-300/25 bg-amber-200/10 px-4 py-3 text-sm font-[var(--font-ui)] text-amber-100 transition hover:bg-amber-200/16 disabled:cursor-not-allowed disabled:opacity-45"
									>
										Draft handoff prompt
									</button>
									<button
										type="button"
										onclick={exportReviewBundle}
										disabled={!selectedThread}
										class="rounded-[18px] border border-white/12 bg-white/6 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-white/10 disabled:cursor-not-allowed disabled:opacity-45"
									>
										Copy review export
									</button>
								</div>
							</section>
						</div>
						{#if phase3Library.checkpoints.length || phase3Library.handoffs.length || phase3Library.reviewExports.length}
							<section class="mt-3 rounded-[22px] border border-white/10 bg-white/[0.04] px-4 py-4">
								<div class="flex flex-wrap items-start justify-between gap-3">
									<div>
										<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
											Phase 3 library
										</p>
										<p class="mt-2 text-xs leading-6 font-[var(--font-ui)] text-stone-300">
											Recent checkpoints, external handoffs, and review exports stay available on this device even when you leave the desk.
										</p>
									</div>
								</div>
								<div class="mt-4 grid gap-3 xl:grid-cols-3">
									<div class="rounded-[18px] border border-white/8 bg-black/18 p-4">
										<p class="text-xs tracking-[0.18em] text-stone-400 uppercase">Checkpoints</p>
										<div class="mt-3 space-y-3 text-sm text-stone-200">
											{#each phase3Library.checkpoints.slice(0, 3) as checkpoint}
												<div class="rounded-[16px] border border-white/8 bg-white/[0.03] px-3 py-3">
													<p class="font-medium text-stone-50">{checkpoint.label}</p>
													<p class="mt-1 text-xs text-stone-400">{checkpoint.sourceThreadLabel}</p>
													{#if checkpoint.note}
														<p class="mt-2 text-xs leading-5 text-stone-300">{checkpoint.note}</p>
													{/if}
												</div>
											{/each}
										</div>
									</div>
									<div class="rounded-[18px] border border-white/8 bg-black/18 p-4">
										<p class="text-xs tracking-[0.18em] text-stone-400 uppercase">Handoffs</p>
										<div class="mt-3 space-y-3 text-sm text-stone-200">
											{#each phase3Library.handoffs.slice(0, 3) as handoff}
												<div class="rounded-[16px] border border-white/8 bg-white/[0.03] px-3 py-3">
													<p class="font-medium text-stone-50">{handoff.title}</p>
													<p class="mt-1 break-all text-xs text-stone-400">{handoff.url}</p>
												</div>
											{/each}
										</div>
									</div>
									<div class="rounded-[18px] border border-white/8 bg-black/18 p-4">
										<p class="text-xs tracking-[0.18em] text-stone-400 uppercase">Review exports</p>
										<div class="mt-3 space-y-3 text-sm text-stone-200">
											{#each phase3Library.reviewExports.slice(0, 3) as reviewExport}
												<div class="rounded-[16px] border border-white/8 bg-white/[0.03] px-3 py-3">
													<p class="font-medium text-stone-50">{reviewExport.threadLabel}</p>
													<p class="mt-1 text-xs text-stone-400">
														{reviewExport.artifactCount} artifacts · {reviewExport.reviewNoteCount} notes · {reviewExport.byteLength} bytes
													</p>
												</div>
											{/each}
										</div>
									</div>
								</div>
							</section>
						{/if}
						</details>
							<div class="mt-3 grid gap-3 lg:grid-cols-[1fr_auto]">
								<textarea
									bind:value={composerText}
									oninput={() => domain.setComposerText(composerText)}
									disabled={Boolean(composerBlockedReason)}
									class="min-h-[120px] rounded-[22px] border border-white/10 bg-white/6 px-4 py-4 text-[15px] leading-7 font-[var(--font-ui)] text-stone-50 placeholder:text-stone-500 focus:border-amber-300/30 focus:ring-0 focus:outline-none"
									placeholder="Continue the selected thread from the browser..."
								></textarea>
								<div class={`grid gap-2 ${selectedActiveTurn ? 'sm:grid-cols-3 lg:grid-cols-1' : 'sm:grid-cols-2 lg:grid-cols-1'}`}>
									<button
										type="button"
										onclick={sendPrompt}
										disabled={Boolean(composerBlockedReason)}
										class="rounded-[22px] bg-amber-300 px-5 py-3 text-sm font-[var(--font-ui)] font-semibold text-amber-950 transition hover:bg-amber-200 disabled:cursor-not-allowed disabled:opacity-45"
										>Send prompt</button
									>
								{#if selectedActiveTurn}
									<button
										type="button"
										onclick={interruptSelectedTurn}
										class="rounded-[22px] border border-rose-300/25 bg-rose-200/10 px-5 py-3 text-sm font-[var(--font-ui)] text-rose-50 transition hover:bg-rose-200/16"
										>Stop turn</button
									>
								{/if}
								<button
									type="button"
									onclick={() => domain.setComposerText('')}
									class="rounded-[22px] border border-white/12 bg-white/6 px-5 py-3 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-white/10"
									>Clear draft</button
								>
							</div>
						</div>
						{#if interimVoiceTranscript}
							<p class="mt-3 rounded-[18px] border border-sky-300/25 bg-sky-300/10 px-4 py-3 text-xs leading-6 font-[var(--font-ui)] text-sky-100">
								Listening now: {interimVoiceTranscript}
							</p>
						{/if}
						{#if composerBlockedReason}
							<p class="mt-3 text-xs leading-6 font-[var(--font-ui)] text-amber-100/82">
								{composerBlockedReason}
							</p>
						{/if}
					</section>
				{:else}
					<section
						class="overflow-hidden rounded-[30px] border border-white/10 bg-[linear-gradient(180deg,_rgba(255,252,247,0.12),_rgba(246,238,227,0.06))] shadow-[0_24px_90px_rgba(0,0,0,0.22)]"
					>
						<div class="border-b border-white/8 px-6 py-6 md:px-8">
							<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase">
								Choose your next thread
							</p>
							<h2 class="mt-3 text-4xl leading-none font-[var(--font-display)] text-stone-50 md:text-5xl">
								Pick one thread and keep moving.
							</h2>
							<p class="mt-4 max-w-3xl text-sm leading-7 font-[var(--font-ui)] text-stone-300">
								Open a saved thread from the rail, or start a fresh one from this browser when you need a clean handoff.
							</p>
							<div class="mt-5 flex flex-wrap gap-3 text-sm font-[var(--font-ui)]">
								<button
									type="button"
									onclick={refresh}
									class="rounded-full border border-white/12 bg-white/6 px-4 py-2 text-stone-100 transition hover:bg-white/10"
								>
									Refresh threads
								</button>
								<button
									type="button"
									onclick={startThread}
									class="rounded-full border border-amber-300/25 bg-amber-200/12 px-4 py-2 text-amber-50 transition hover:bg-amber-200/20"
								>
									Start new thread
								</button>
							</div>
						</div>
						<div class="grid gap-4 px-6 py-6 md:grid-cols-3 md:px-8">
							<div class="rounded-[24px] border border-white/8 bg-black/16 p-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">1. Browse</p>
								<p class="mt-3 text-xl font-[var(--font-display)] text-stone-50">Scan the saved thread rail.</p>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">Open a past thread when you want context, or ignore the rail if you just need a fresh start.</p>
							</div>
							<div class="rounded-[24px] border border-white/8 bg-black/16 p-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">2. Open</p>
								<p class="mt-3 text-xl font-[var(--font-display)] text-stone-50">Bring one thread into focus.</p>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">The browser loads the transcript, shows blockers, and tells you if that thread is already open somewhere else.</p>
							</div>
							<div class="rounded-[24px] border border-white/8 bg-black/16 p-4">
								<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">3. Continue</p>
								<p class="mt-3 text-xl font-[var(--font-display)] text-stone-50">Reply, resume, or start fresh.</p>
								<p class="mt-3 text-sm leading-6 font-[var(--font-ui)] text-stone-300">Use the browser for quick replies and approvals, or jump into the composer when you need the next full prompt.</p>
							</div>
						</div>
					</section>
				{/if}

				<details class="rounded-[26px] border border-white/10 bg-black/18 p-4 shadow-inner shadow-black/25">
					<summary class="flex cursor-pointer list-none items-center justify-between gap-3 [&::-webkit-details-marker]:hidden">
						<div>
							<p class="text-[0.68rem] font-[var(--font-ui)] tracking-[0.18em] text-stone-400 uppercase">
								Recent activity
							</p>
							<p class="mt-2 text-sm font-[var(--font-ui)] text-stone-300">
								Open this when you want the raw event feed.
							</p>
						</div>
						<span class="rounded-full border border-white/10 bg-white/6 px-3 py-1.5 text-xs font-[var(--font-ui)] text-stone-200">
							{$workspaceState.events.length} event{$workspaceState.events.length === 1 ? '' : 's'}
						</span>
					</summary>
					<div class="mt-3 flex flex-wrap gap-2 text-xs font-[var(--font-ui)] text-stone-300">
						<button
							type="button"
							onclick={() => (activityFilter = 'all')}
							class={`rounded-full px-3 py-1.5 transition ${activityFilter === 'all' ? 'bg-stone-100 text-stone-950' : 'bg-white/6 text-stone-200 hover:bg-white/10'}`}
						>
							All activity ({activityFilterCount('all')})
						</button>
						<button
							type="button"
							onclick={() => (activityFilter = 'selected')}
							disabled={!selectedThread}
							class={`rounded-full px-3 py-1.5 transition ${activityFilter === 'selected' ? 'bg-teal-200 text-stone-950' : 'bg-white/6 text-stone-200 hover:bg-white/10'} disabled:cursor-not-allowed disabled:opacity-45`}
						>
							This thread ({activityFilterCount('selected')})
						</button>
						<button
							type="button"
							onclick={() => (activityFilter = 'blockers')}
							class={`rounded-full px-3 py-1.5 transition ${activityFilter === 'blockers' ? 'bg-amber-200 text-amber-950' : 'bg-white/6 text-stone-200 hover:bg-white/10'}`}
						>
							Blockers ({activityFilterCount('blockers')})
						</button>
					</div>
					<div class="mt-3 max-h-[15rem] space-y-2 overflow-y-auto pr-1">
						{#if !filteredEvents.length}
							<div
								class="rounded-[20px] border border-dashed border-white/10 bg-white/4 px-4 py-4 text-sm font-[var(--font-ui)] text-stone-400"
							>
								{activityFilter === 'all'
									? 'Resume a thread or answer a pending request to populate this log.'
									: activityFilter === 'selected'
										? 'Open a thread and keep working here to isolate its event feed.'
										: 'No blocker-style events are in the recent feed right now.'}
							</div>
						{:else}
							{#each filteredEvents as event}
								<div
									class="rounded-[20px] border border-white/8 bg-white/5 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-300"
								>
									<div
										class="mb-1 flex items-center justify-between gap-3 text-[11px] tracking-[0.16em] text-stone-500 uppercase"
									>
										<span>{event.method}</span>
										<span
											>{new Intl.DateTimeFormat('en-US', {
												hour: 'numeric',
												minute: '2-digit',
												second: '2-digit'
											}).format(event.at)}</span
										>
									</div>
									<p>{event.summary}</p>
								</div>
							{/each}
						{/if}
					</div>
				</details>
			</main>
		</div>
		{#if selectedThread}
			<section
				class="fixed right-3 bottom-3 left-3 z-30 rounded-[24px] border border-white/10 bg-stone-950/96 px-4 py-3 text-stone-50 shadow-[0_18px_40px_rgba(0,0,0,0.24)] backdrop-blur md:hidden"
				style="padding-bottom: calc(env(safe-area-inset-bottom, 0px) + 0.75rem);"
			>
				<div class="flex items-start justify-between gap-3">
					<div class="min-w-0">
						<p class="text-[0.62rem] font-[var(--font-ui)] tracking-[0.22em] text-stone-400 uppercase">
							Mobile companion
						</p>
						<p class="mt-2 truncate text-base leading-6 font-[var(--font-display)] text-stone-50">
							{threadDisplayLabel(selectedThread)}
						</p>
						<p class="mt-1 text-xs leading-5 font-[var(--font-ui)] text-stone-300">
							{currentSignalLabel()} · {selectedPendingCount()} pending · {$workspaceState.connectionPhase}
						</p>
					</div>
					<span class={`rounded-full px-2.5 py-1 text-[10px] font-[var(--font-ui)] tracking-[0.18em] uppercase ${currentSignalTone()}`}>
						{currentSignalLabel()}
					</span>
				</div>
				<div class="mt-3 grid grid-cols-[1fr_auto] gap-2">
					<button
						type="button"
						onclick={runMobilePrimaryAction}
						class="rounded-[18px] bg-amber-300 px-4 py-3 text-sm font-[var(--font-ui)] font-semibold text-amber-950 transition hover:bg-amber-200"
					>
						{mobilePrimaryLabel}
					</button>
					<button
						type="button"
						onclick={() => responderSection?.scrollIntoView({ behavior: 'smooth', block: 'start' })}
						class="rounded-[18px] border border-white/12 bg-white/6 px-4 py-3 text-sm font-[var(--font-ui)] text-stone-50 transition hover:bg-white/10"
					>
						Jump
					</button>
				</div>
				{#if composerBlockedReason}
					<p class="mt-2 text-[11px] leading-5 font-[var(--font-ui)] text-amber-100/82">
						{composerBlockedReason}
					</p>
				{/if}
			</section>
		{/if}
	</div>
</div>
