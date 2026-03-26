---
name: browser-ui-review
description: Run a browser-first UI/UX review with concrete evidence. Use when
  the user wants feedback on a web UI, layout polish, usability issues,
  responsiveness, visual regressions, or wants Claude/Gemini to critique the
  interface instead of only reading code.
---

# Browser UI Review

Use this skill for browser-based product critique, not static code review.

## When to use it

- The user asks what is wrong with a UI or why it feels bad.
- The task is frontend UX polish, visual QA, responsive cleanup, or
  interaction review.
- The user wants Claude/Gemini to act like outside reviewers.

## Core rule

Do not rely on source inspection alone for UI/UX feedback when the page can be
run locally.
Open the UI, inspect the real experience, and gather evidence before
recommending changes.

## Workflow

1. Find or start the relevant app/page.
2. Open it in the browser tool.
3. Check the page at desktop size first, then a narrow/mobile width if layout
   changes matter.
4. Note visible issues, interaction rough edges, console/runtime errors,
   empty states, spacing problems, and unclear hierarchy.
5. If useful, capture screenshots or specific DOM text as evidence.
6. Ask 1-2 external agents for critique, usually one Claude model and one
   Gemini model, in read-only mode.
7. Synthesize the feedback into a short prioritized list.
8. If the user wants fixes, implement them and re-check in the browser.

## What to look for

- Visual hierarchy: can you tell what matters first?
- Layout rhythm: spacing, alignment, density, awkward empty areas.
- Readability: font sizing, contrast, line length, labels, button copy.
- Interaction clarity: hover/focus states, disabled states, affordances,
  loading feedback.
- Responsiveness: overflow, cramped controls, broken wrapping, unusable mobile
  layouts.
- Product feel: does it look generic, inconsistent, or unfinished?
- Errors: console noise, failed requests, missing assets, broken hydration or
  rendering.

## Agent usage

Prefer agents after you have already looked at the real page yourself.
Give them concrete context:

- URL or route reviewed
- what viewport(s) you checked
- short summary of what the product/page is supposed to do
- screenshots or observed issues when available
- explicit instruction to critique UX and visual hierarchy, not architecture

Recommended pattern:

- Claude: stronger wording and interaction critique
- Gemini: stronger breadth / alternate perspective

Keep agents read-only unless the user explicitly wants one to implement
changes.

## Review prompt shape

Use a prompt like:

```text
Review this UI as a product designer and frontend critic.

Context:
- App/page: <what this screen is for>
- Route/URL: <url>
- Viewports checked: desktop and mobile
- Constraints: preserve the existing design system unless clearly broken

Focus on:
- top 5 UI/UX problems hurting clarity or quality
- concrete suggestions to fix each one
- anything that feels generic, confusing, visually noisy, or inconsistent

Do not review backend architecture. Keep the feedback evidence-based and
specific to the interface.
```

## Output expectations

When reporting findings back to the user:

- lead with the highest-impact UI issues
- separate confirmed browser-observed problems from agent suggestions
- prefer specific fixes over vague design language
- if you make changes, include what you re-checked in the browser afterward

## References

- Prompt examples: `references/prompts.md`
