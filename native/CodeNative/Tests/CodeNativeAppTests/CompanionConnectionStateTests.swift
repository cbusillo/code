import XCTest
@testable import CodeNativeApp

final class CompanionConnectionStateTests: XCTestCase {
    func testResolvePairRequiredFromUnauthorizedError() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .disconnected,
            statusLine: "Disconnected",
            lastError: "HTTP upgrade failed with 401 Unauthorized",
            hasPendingApproval: false
        )

        XCTAssertEqual(resolved, .pairRequired)
    }

    func testResolveApprovalPendingTakesPriority() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .connected,
            statusLine: "Connected",
            lastError: nil,
            hasPendingApproval: true
        )

        XCTAssertEqual(resolved, .approvalPending)
    }

    func testResolveConnectingDefaultsToDiscovering() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .connecting,
            statusLine: "Connecting...",
            lastError: nil,
            hasPendingApproval: false
        )

        XCTAssertEqual(resolved, .discovering)
    }

    func testResolveConnectingWithReconnectHintUsesReconnecting() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .connecting,
            statusLine: "Reconnecting...",
            lastError: nil,
            hasPendingApproval: false
        )

        XCTAssertEqual(resolved, .reconnecting)
    }

    func testResolveDisconnectedDefaultsToOffline() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .disconnected,
            statusLine: "Disconnected",
            lastError: nil,
            hasPendingApproval: false
        )

        XCTAssertEqual(resolved, .offline)
    }

    func testResolveConnectedDefaultsToConnected() {
        let resolved = CompanionConnectionState.resolve(
            connectionState: .connected,
            statusLine: "Connected",
            lastError: nil,
            hasPendingApproval: false
        )

        XCTAssertEqual(resolved, .connected)
    }
}
