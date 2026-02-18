import Foundation
import XCTest
@testable import CodeNativeApp

final class LocalBackendRuntimeSupervisorTests: XCTestCase {
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
