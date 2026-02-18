import Foundation
import XCTest
@testable import CodeNativeApp

final class LocalBackendRuntimeSupervisorTests: XCTestCase {
    @MainActor
    func testCanRotateCompanionSessionTokenFalseWhenEnvironmentTokenConfigured() {
        let supervisor = LocalBackendRuntimeSupervisor(
            arguments: ["CodeNativeApp"],
            environment: [
                "CODE_NATIVE_DISABLE_MANAGED_BACKEND": "true",
                "CODE_NATIVE_COMPANION_TOKEN": "managed-token",
            ]
        )

        XCTAssertFalse(supervisor.canRotateCompanionSessionToken)
    }

    @MainActor
    func testCanRotateCompanionSessionTokenTrueWhenTokenIsGenerated() {
        let supervisor = LocalBackendRuntimeSupervisor(
            arguments: ["CodeNativeApp"],
            environment: ["CODE_NATIVE_DISABLE_MANAGED_BACKEND": "true"]
        )

        XCTAssertTrue(supervisor.canRotateCompanionSessionToken)
    }

    func testNormalizeCompanionSessionTokensRemovesDuplicatesAndWhitespace() {
        XCTAssertEqual(
            LocalBackendRuntimeSupervisor.normalizeCompanionSessionTokens([
                " token-a ",
                "",
                "token-b",
                "token-a",
                "   ",
            ]),
            ["token-a", "token-b"]
        )
    }

    func testNormalizeCompanionPairingEntriesDropsInvalidAndSortsStable() {
        let entries = [
            CompanionPairingEntry(
                id: "b-id",
                label: " Tablet ",
                sessionToken: " token-b ",
                createdAtUnixMs: 20,
                expiresAtUnixMs: 2_000,
                revokedAtUnixMs: nil
            ),
            CompanionPairingEntry(
                id: "",
                label: "Invalid",
                sessionToken: "token-invalid",
                createdAtUnixMs: 10,
                expiresAtUnixMs: 1_000,
                revokedAtUnixMs: nil
            ),
            CompanionPairingEntry(
                id: "a-id",
                label: "Phone",
                sessionToken: "token-a",
                createdAtUnixMs: 10,
                expiresAtUnixMs: 1_000,
                revokedAtUnixMs: nil
            ),
            CompanionPairingEntry(
                id: "a-id",
                label: "Duplicate",
                sessionToken: "token-dup",
                createdAtUnixMs: 30,
                expiresAtUnixMs: 3_000,
                revokedAtUnixMs: nil
            ),
        ]

        let normalized = LocalBackendRuntimeSupervisor.normalizeCompanionPairingEntries(entries)

        XCTAssertEqual(normalized.map(\.id), ["a-id", "b-id"])
        XCTAssertEqual(normalized.map(\.sessionToken), ["token-a", "token-b"])
        XCTAssertEqual(normalized.map(\.label), ["Phone", "Tablet"])
        XCTAssertEqual(normalized.map(\.expiresAtUnixMs), [1_000, 2_000])
    }

    func testActiveCompanionSessionTokensExcludeRevokedAndExpiredEntries() {
        let entries = [
            CompanionPairingEntry(
                id: "active",
                label: "Active",
                sessionToken: "active-token",
                createdAtUnixMs: 10,
                expiresAtUnixMs: 5_000,
                revokedAtUnixMs: nil
            ),
            CompanionPairingEntry(
                id: "expired",
                label: "Expired",
                sessionToken: "expired-token",
                createdAtUnixMs: 10,
                expiresAtUnixMs: 50,
                revokedAtUnixMs: nil
            ),
            CompanionPairingEntry(
                id: "revoked",
                label: "Revoked",
                sessionToken: "revoked-token",
                createdAtUnixMs: 10,
                expiresAtUnixMs: 5_000,
                revokedAtUnixMs: 20
            ),
        ]

        let activeTokens = LocalBackendRuntimeSupervisor.activeCompanionSessionTokens(
            localSessionToken: "local-token",
            pairingEntries: entries,
            referenceUnixMs: 100
        )

        XCTAssertEqual(activeTokens, ["local-token", "active-token"])
    }

    func testCompanionPairingEntryDecodeBackfillsMissingExpiry() throws {
        let payload = """
        {
          "id": "pairing-a",
          "label": "Phone",
          "sessionToken": "token-a",
          "createdAtUnixMs": 1000,
          "revokedAtUnixMs": null
        }
        """

        let entry = try JSONDecoder().decode(
            CompanionPairingEntry.self,
            from: Data(payload.utf8)
        )

        XCTAssertGreaterThan(entry.expiresAtUnixMs, entry.createdAtUnixMs)
    }

    func testShouldManageBackendDisabledForBenchmarkFixtureEnvironment() {
        XCTAssertFalse(
            LocalBackendRuntimeSupervisor.shouldManageBackend(
                arguments: ["CodeNativeApp"],
                environment: ["CODE_NATIVE_BENCHMARK_FIXTURE": "/tmp/fixture.json"]
            )
        )
    }

    func testShouldManageBackendDisabledByExplicitFlag() {
        XCTAssertFalse(
            LocalBackendRuntimeSupervisor.shouldManageBackend(
                arguments: ["CodeNativeApp"],
                environment: ["CODE_NATIVE_DISABLE_MANAGED_BACKEND": "true"]
            )
        )
    }

    func testResolveBinaryURLPrefersExplicitOverride() throws {
        let temporaryDirectory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer {
            try? FileManager.default.removeItem(at: temporaryDirectory)
        }

        let binaryURL = temporaryDirectory.appendingPathComponent("code")
        try "#!/bin/sh\necho managed\n".write(to: binaryURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: NSNumber(value: Int16(0o755))],
            ofItemAtPath: binaryURL.path
        )

        let resolved = LocalBackendRuntimeSupervisor.resolveBinaryURL(
            environment: [
                "CODE_NATIVE_BACKEND_BINARY": binaryURL.path,
                "PATH": "",
            ],
            currentDirectoryURL: temporaryDirectory,
            executableDirectoryURL: temporaryDirectory,
            bundleResourceURL: nil,
            fileManager: .default
        )

        XCTAssertEqual(resolved?.standardizedFileURL.path, binaryURL.standardizedFileURL.path)
    }

    func testResolveBinaryURLReturnsNilWhenNoCandidatesExist() throws {
        let temporaryDirectory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        try FileManager.default.createDirectory(at: temporaryDirectory, withIntermediateDirectories: true)
        defer {
            try? FileManager.default.removeItem(at: temporaryDirectory)
        }

        let resolved = LocalBackendRuntimeSupervisor.resolveBinaryURL(
            environment: ["PATH": ""],
            currentDirectoryURL: temporaryDirectory,
            executableDirectoryURL: temporaryDirectory,
            bundleResourceURL: nil,
            fileManager: .default
        )

        XCTAssertNil(resolved)
    }

    func testResolveBinaryURLPrefersBundledBackendPath() throws {
        let temporaryDirectory = URL(fileURLWithPath: NSTemporaryDirectory())
            .appendingPathComponent(UUID().uuidString, isDirectory: true)
        let resourceDirectory = temporaryDirectory.appendingPathComponent("Resources", isDirectory: true)
        let bundledBackendDirectory = resourceDirectory.appendingPathComponent("backend", isDirectory: true)
        try FileManager.default.createDirectory(at: bundledBackendDirectory, withIntermediateDirectories: true)
        defer {
            try? FileManager.default.removeItem(at: temporaryDirectory)
        }

        let bundledBinaryURL = bundledBackendDirectory.appendingPathComponent("code")
        try "#!/bin/sh\necho bundled\n".write(to: bundledBinaryURL, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes(
            [.posixPermissions: NSNumber(value: Int16(0o755))],
            ofItemAtPath: bundledBinaryURL.path
        )

        let resolved = LocalBackendRuntimeSupervisor.resolveBinaryURL(
            environment: ["PATH": ""],
            currentDirectoryURL: temporaryDirectory,
            executableDirectoryURL: temporaryDirectory,
            bundleResourceURL: resourceDirectory,
            fileManager: .default
        )

        XCTAssertNotNil(resolved)
        XCTAssertEqual(resolved?.lastPathComponent, "code")
        XCTAssertEqual(
            resolved?.deletingLastPathComponent().lastPathComponent.lowercased(),
            "backend"
        )
    }

    func testReserveLoopbackPortReturnsNonZeroPort() {
        let port = LocalBackendRuntimeSupervisor.reserveLoopbackPort()
        XCTAssertNotNil(port)
        XCTAssertGreaterThan(port ?? 0, 0)
    }

    func testResolveCompanionSessionTokenUsesConfiguredValue() {
        let token = LocalBackendRuntimeSupervisor.resolveCompanionSessionToken(
            environment: ["CODE_NATIVE_COMPANION_TOKEN": "paired-token"]
        )
        XCTAssertEqual(token, "paired-token")
    }

    func testResolveCompanionSessionTokenGeneratesFallback() {
        let token = LocalBackendRuntimeSupervisor.resolveCompanionSessionToken(environment: [:])
        XCTAssertNotNil(token)
        XCTAssertGreaterThan(token?.count ?? 0, 20)
    }
}
