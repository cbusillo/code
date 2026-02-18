# Every Code Companion Metadata Pack

Use this as the source of truth for TestFlight and App Store Connect listing
fields.

## Naming

- `App name`: `Every Code Companion`
- `Subtitle`: `Unofficial native client`
- `Internal/TestFlight name`: `Every Code Companion Beta`

## Legal Attribution

Required line for App Store listing, in-app settings, and README:

`Every Code Companion is an independent project and is not affiliated with or`
`endorsed by Every Code.`

## App Store Metadata

- `Promotional text`:
  `A fast native interface for Every Code workflows with transcript parity,`
  `task visibility, approvals, and benchmarked reliability.`
- `Short description`:
  `Native companion app for Every Code with rich transcript UX, approvals,`
  `slash commands, mentions, and session organization.`
- `Keywords`:
  `developer,ai,coding,assistant,terminal,workflow,review,automation`
- `Support URL`: `<set-your-support-url>`
- `Marketing URL`: `<optional>`
- `Privacy Policy URL`: `<set-your-privacy-url>`

## Long Description

`Every Code Companion is a native app for Every Code workflows on Apple`
`platforms.`

`It provides a polished transcript experience with strong readability for`
`planning, execution, review, approvals, and recovery flows.`

`Key capabilities include slash launcher workflows, mention-based context`
`insertion, request-input cards, IDE handoff controls, and deterministic UI`
`benchmarking for quality gates.`

`Every Code Companion is an independent project and is not affiliated with or`
`endorsed by Every Code.`

## TestFlight Notes Template

- `What to test`:
  `Transcript readability and activity cards, approvals, slash launcher,`
  `mentions, and request-input card behavior.`
- `Feedback focus`:
  `Report any stuck runtime states, missing history pages, keyboard issues,`
  `or incorrect IDE open behavior.`

## Versioned Release Notes Template

`This build improves native workflow parity, transcript reliability, and`
`session ergonomics.`

`Please test approvals, slash commands, context mentions, and long-session`
`scrollback behavior.`

## Submission Checklist

- Confirm display name is `Every Code Companion`.
- Confirm bundle identifier is `com.shinycomputers.everycodecompanion`.
- Confirm legal attribution appears in settings and listing text.
- Confirm screenshots match deterministic benchmark artifacts.
- Confirm privacy/support URLs are set before submission.

## CI TestFlight Workflow

Use `.github/workflows/native-ios-testflight.yml` to build a signed IPA and
upload to TestFlight.

Required repository secrets:

- `IOS_DIST_CERT_P12_BASE64`: Base64-encoded Apple Distribution `.p12`.
- `IOS_DIST_CERT_PASSWORD`: Password for the distribution `.p12`.
- `IOS_APPSTORE_PROFILE_BASE64`: Base64-encoded App Store provisioning profile.
- `IOS_APPSTORE_PROFILE_NAME`: Provisioning profile name used during export.
- `APP_STORE_CONNECT_KEY_ID`: App Store Connect API key id.
- `APP_STORE_CONNECT_ISSUER_ID`: App Store Connect API issuer id.
- `APP_STORE_CONNECT_PRIVATE_KEY`: Raw `.p8` API private key contents.

Workflow notes:

- Team ID is tracked in workflow as `MM5YXC7T6E`.
- The job always emits an IPA artifact (`EveryCodeCompanion-ipa`).
- Push tags matching `ios-v*` for automatic release-driven uploads.
- Set `upload_to_testflight = true` when dispatching to publish to TestFlight.

Tag helper:

- `scripts/release-ios-tag.sh 1.4.0` creates and pushes `ios-v1.4.0`.
