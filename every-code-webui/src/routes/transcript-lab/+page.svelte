<script lang="ts">
	import TranscriptThreadView from '$lib/components/transcript-thread-view.svelte';
	import { transcriptCells } from '$lib/transcript-fixture';
	import type { TranscriptSessionBanner } from '$lib/transcript-model';

	const sessionBanner: TranscriptSessionBanner = {
		state: 'active_elsewhere',
		title: 'Live turn is attached elsewhere.',
		detail:
			'This lab uses the real transcript component with fixture data so review-board and live-follow changes can be inspected without depending on a hydrated app-server thread.',
		primaryAction: 'take_over',
		primaryLabel: 'Take over here'
	};

	const badges = ['~/Developer/code', '12 turns', 'branch main', '1 pending action'];

	function handleSessionAction() {
		return;
	}

	function handleApprovalAction() {
		return;
	}

	function handleQuestionOptionSelect() {
		return;
	}

	function handleQuestionCustomValue() {
		return;
	}

	function handleQuestionSubmit() {
		return;
	}

	function handleQuestionFocus() {
		return;
	}
</script>

<svelte:head>
	<title>Transcript Lab</title>
	<meta name="description" content="Fixture-backed transcript lab for the shared-session browser companion." />
</svelte:head>

<div class="min-h-screen bg-[radial-gradient(circle_at_top,_rgba(43,84,87,0.3),_transparent_36%),linear-gradient(180deg,_#071012_0%,_#0d1718_45%,_#120f0d_100%)] px-4 py-6 text-stone-100 lg:px-6">
	<div class="mx-auto max-w-[1400px] space-y-5">
		<section class="rounded-[28px] border border-white/10 bg-[linear-gradient(135deg,_rgba(255,255,255,0.08),_rgba(255,255,255,0.03))] px-5 py-5 shadow-[0_20px_80px_rgba(0,0,0,0.32)] backdrop-blur md:px-7 md:py-6">
			<p class="text-[0.7rem] font-[var(--font-ui)] tracking-[0.28em] text-amber-200/72 uppercase">
				EveryCode WebUI · Transcript lab
			</p>
			<h1 class="mt-3 max-w-4xl text-[2.8rem] leading-[0.96] font-[var(--font-display)] tracking-[-0.04em] text-stone-50 md:text-[3.4rem]">
				Fixture-backed validation for the real transcript surface.
			</h1>
			<p class="mt-4 max-w-3xl text-sm leading-7 font-[var(--font-ui)] text-stone-300 md:text-[15px]">
				This route exists so review-board changes, live-follow polish, and future transcript work can be visually verified against the actual shared-session transcript component instead of only the older prototype page.
			</p>
		</section>

		<TranscriptThreadView
			threadId="fixture-thread-lab"
			title="Prototype the continuity-first WebUI"
			description="The real transcript component, rendered with the existing golden fixture so review flows can be inspected without a live backend dependency."
			badges={badges}
			cells={transcriptCells}
			sessionBanner={sessionBanner}
			canApprove={true}
			onSessionAction={handleSessionAction}
			onApprovalAction={handleApprovalAction}
			onQuestionOptionSelect={handleQuestionOptionSelect}
			onQuestionCustomValue={handleQuestionCustomValue}
			onQuestionSubmit={handleQuestionSubmit}
			onQuestionFocus={handleQuestionFocus}
		/>
	</div>
</div>
