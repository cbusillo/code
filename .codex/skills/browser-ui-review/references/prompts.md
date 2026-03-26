# Browser UI Review Prompts

## Quick critique prompt

```text
Review this UI as an experienced product designer.

Screen purpose: <purpose>
Route/URL: <url>
Observed in browser at: desktop and mobile widths

Give me:
1. The top 5 problems with the current UI/UX
2. Why each problem matters
3. One concrete fix for each problem

Focus on hierarchy, spacing, readability, responsiveness, affordances, and polish.
Avoid backend or code-architecture commentary.
```

## Polish pass prompt

```text
I want a blunt UI polish review.

This page works, but it feels mediocre.
Tell me what makes it feel generic, awkward, noisy, or low-confidence.

Then propose a tighter visual direction while preserving the product's purpose.
Keep feedback specific and implementation-oriented.
```

## Compare-before-after prompt

```text
Compare this updated UI against the prior version I describe below.

Prior issues:
- <issue>
- <issue>

Tell me:
- which issues are actually improved
- what still feels off
- the next 3 highest-value polish changes
```
