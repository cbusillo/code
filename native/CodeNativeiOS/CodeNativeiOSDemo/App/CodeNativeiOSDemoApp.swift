import Darwin
import Foundation
import OSLog
import SwiftUI

@main
struct CodeNativeiOSDemoApp: App {
    private static let sharedCompanionRegistryKey = "code_native_shared_companion_backends_v1"
    private static let sharedCompanionStaleWindowMs: UInt64 = 7 * 24 * 60 * 60 * 1_000
    private static let sharedCompanionSyncIntervalNs: UInt64 = 5_000_000_000

    @StateObject private var store = SessionMirrorStore(endpointAccessPolicy: .anyHost)
    @StateObject private var networkPathChangeMonitor = NetworkPathChangeMonitor()
    @State private var sharedCompanionRecords: [SharedCompanionBackendRecord] = []
    @State private var sharedCompanionSyncTask: Task<Void, Never>?
    @AppStorage("code_native_auto_connect_shared_companion")
    private var autoConnectSharedCompanion = true
    @AppStorage("code_native_preferred_shared_companion_id")
    private var preferredSharedCompanionBackendID = ""

    private let logger = Logger(subsystem: "com.every.code.native", category: "shared-companion-ios")

    private var sharedCompanionBackends: [SharedCompanionBackend] {
        sharedCompanionRecords.map { record in
            SharedCompanionBackend(
                id: record.id,
                displayName: record.displayName,
                endpoint: record.endpoint,
                backendVersion: record.backendVersion,
                minimumSupportedClientVersion: record.minimumSupportedClientVersion,
                updatedAtUnixMs: record.updatedAtUnixMs
            )
        }
    }

    var body: some Scene {
        WindowGroup {
            ContentView(
                store: store,
                sharedCompanionBackends: sharedCompanionBackends,
                refreshSharedCompanionBackends: {
                    refreshSharedCompanionBackends()
                },
                onSharedCompanionPreferencesChanged: {
                    applySharedCompanionSelection()
                }
            )
                .task {
                    NSUbiquitousKeyValueStore.default.synchronize()
                    refreshSharedCompanionBackends()
                    applySharedCompanionSelection()
                    startSharedCompanionSyncLoopIfNeeded()
                }
                .onChange(of: autoConnectSharedCompanion) { _, _ in
                    applySharedCompanionSelection()
                }
                .onChange(of: preferredSharedCompanionBackendID) { _, _ in
                    applySharedCompanionSelection()
                }
                .onReceive(NotificationCenter.default.publisher(for: NSUbiquitousKeyValueStore.didChangeExternallyNotification)) { _ in
                    refreshSharedCompanionBackends()
                    applySharedCompanionSelection()
                }
                .onReceive(networkPathChangeMonitor.$changeCount.dropFirst()) { _ in
                    applySharedCompanionSelection()
                }
                .onOpenURL { url in
                    _ = store.importCompanionPairingCode(url.absoluteString)
                }
        }
    }

    private func startSharedCompanionSyncLoopIfNeeded() {
        guard sharedCompanionSyncTask == nil else {
            return
        }

        sharedCompanionSyncTask = Task {
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: Self.sharedCompanionSyncIntervalNs)
                guard !Task.isCancelled else {
                    return
                }

                await MainActor.run {
                    NSUbiquitousKeyValueStore.default.synchronize()
                    refreshSharedCompanionBackends()
                    applySharedCompanionSelection()
                }
            }
        }
    }

    private func refreshSharedCompanionBackends(
        store kvStore: NSUbiquitousKeyValueStore = .default,
        nowUnixMs: UInt64 = nowUnixTimeMilliseconds()
    ) {
        guard let raw = kvStore.string(forKey: Self.sharedCompanionRegistryKey),
              let data = raw.data(using: .utf8),
              let records = try? JSONDecoder().decode([SharedCompanionBackendRecord].self, from: data)
        else {
            sharedCompanionRecords = []
            logger.debug("Shared companion registry is empty on iOS")
            return
        }

        let staleCutoff = nowUnixMs > Self.sharedCompanionStaleWindowMs
            ? nowUnixMs - Self.sharedCompanionStaleWindowMs
            : 0
        sharedCompanionRecords = records
            .filter { $0.updatedAtUnixMs >= staleCutoff }
            .sorted(by: { $0.updatedAtUnixMs > $1.updatedAtUnixMs })
        logger.debug("Loaded \(sharedCompanionRecords.count, privacy: .public) shared companion records on iOS")
    }

    private func applySharedCompanionSelection() {
        guard autoConnectSharedCompanion,
              let record = selectedSharedCompanionRecord()
        else {
            return
        }

        let endpoint = preferredEndpoint(for: record)
        let trimmedEndpoint = endpoint.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedEndpoint = trimmedEndpoint.isEmpty ? SessionMirrorStore.defaultEndpoint : trimmedEndpoint
        let trimmedToken = record.sessionToken.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedToken.isEmpty else {
            return
        }

        let normalizedLANEndpoint: String?
        if let lanEndpoint = record.lanEndpoint?.trimmingCharacters(in: .whitespacesAndNewlines),
           !lanEndpoint.isEmpty {
            normalizedLANEndpoint = lanEndpoint
        } else {
            normalizedLANEndpoint = nil
        }

        let endpointChanged = store.endpoint != normalizedEndpoint
        let tokenChanged = store.companionSessionToken != trimmedToken
        let lanEndpointChanged = store.companionLANEndpoint != normalizedLANEndpoint
        guard endpointChanged || tokenChanged || lanEndpointChanged else {
            return
        }

        let shouldReconnect = store.connectionState == .connected || store.connectionState == .connecting
        if shouldReconnect {
            store.disconnect()
        }

        store.endpoint = normalizedEndpoint
        store.companionSessionToken = trimmedToken
        store.companionLANEndpoint = normalizedLANEndpoint
        logger.info("Applying shared companion backend \(record.id, privacy: .public) on iOS")
        Task {
            await store.connect()
        }
    }

    private func selectedSharedCompanionRecord() -> SharedCompanionBackendRecord? {
        let clientVersion = AppVersion.currentComparableVersion()
        let compatibleRecords = sharedCompanionRecords.filter { record in
            guard let minimumVersion = VersionComparator.normalizedVersionString(record.minimumSupportedClientVersion) else {
                return true
            }

            return VersionComparator.isAtLeast(clientVersion, minimum: minimumVersion)
        }

        let trimmedPreferredID = preferredSharedCompanionBackendID.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmedPreferredID.isEmpty,
           let preferred = compatibleRecords.first(where: { $0.id == trimmedPreferredID }) {
            return preferred
        }

        return compatibleRecords.first
    }

    private func preferredEndpoint(for record: SharedCompanionBackendRecord) -> String {
        let trimmedPrimaryEndpoint = record.endpoint.trimmingCharacters(in: .whitespacesAndNewlines)

        let primaryEndpointPrefersLAN =
            !trimmedPrimaryEndpoint.isEmpty
                && endpointHostIsPrivateLANIPv4(trimmedPrimaryEndpoint)

        let localLANAddresses = localPrivateLANIPv4Addresses()
        if primaryEndpointPrefersLAN,
           let lanEndpoint = record.lanEndpoint,
           endpointIsOnSamePrivateLANSubnet(lanEndpoint, localLANAddresses: localLANAddresses) {
            return lanEndpoint
        }

        if !trimmedPrimaryEndpoint.isEmpty {
            return trimmedPrimaryEndpoint
        }

        if let tailscaleEndpoint = record.tailscaleEndpoint {
            let trimmedTailscaleEndpoint = tailscaleEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedTailscaleEndpoint.isEmpty {
                return trimmedTailscaleEndpoint
            }
        }

        if let lanEndpoint = record.lanEndpoint {
            let trimmedLANEndpoint = lanEndpoint.trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmedLANEndpoint.isEmpty {
                return trimmedLANEndpoint
            }
        }

        return record.endpoint
    }

    private func localPrivateLANIPv4Addresses() -> [String] {
        allIPv4Addresses { interfaceName, ipAddress in
            let lowerInterfaceName = interfaceName.lowercased()
            if isTailscaleInterface(lowerInterfaceName) {
                return false
            }
            return isPrivateLANIPv4Address(ipAddress)
        }
    }

    private func endpointHostIsPrivateLANIPv4(_ endpoint: String) -> Bool {
        guard let endpointURL = URL(string: endpoint),
              let endpointHost = endpointURL.host,
              let octets = ipv4Octets(from: endpointHost)
        else {
            return false
        }

        if octets.0 == 10 || octets.0 == 192 && octets.1 == 168 {
            return true
        }

        if octets.0 == 172,
           (16...31).contains(octets.1) {
            return true
        }

        return false
    }

    private func isTailscaleInterface(_ lowerInterfaceName: String) -> Bool {
        lowerInterfaceName.hasPrefix("utun") || lowerInterfaceName.contains("tailscale")
    }

    private func allIPv4Addresses(where predicate: (String, String) -> Bool) -> [String] {
        var interfaces: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&interfaces) == 0,
              let firstInterface = interfaces
        else {
            return []
        }
        defer {
            freeifaddrs(interfaces)
        }

        var addresses: [String] = []
        var seenAddresses: Set<String> = []
        var pointer: UnsafeMutablePointer<ifaddrs>? = firstInterface
        while let interface = pointer {
            defer {
                pointer = interface.pointee.ifa_next
            }

            let interfaceName = String(cString: interface.pointee.ifa_name)
            let flags = Int32(interface.pointee.ifa_flags)
            guard (flags & IFF_UP) != 0,
                  (flags & IFF_LOOPBACK) == 0,
                  let addressPointer = interface.pointee.ifa_addr,
                  addressPointer.pointee.sa_family == UInt8(AF_INET)
            else {
                continue
            }

            var socketAddress = addressPointer.pointee
            var hostBuffer = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            let conversionResult = getnameinfo(
                &socketAddress,
                socklen_t(addressPointer.pointee.sa_len),
                &hostBuffer,
                socklen_t(hostBuffer.count),
                nil,
                0,
                NI_NUMERICHOST
            )
            guard conversionResult == 0 else {
                continue
            }

            let hostBytes = hostBuffer.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
            let ipAddress = String(decoding: hostBytes, as: UTF8.self)
            if !ipAddress.isEmpty,
               predicate(interfaceName, ipAddress),
               !seenAddresses.contains(ipAddress) {
                seenAddresses.insert(ipAddress)
                addresses.append(ipAddress)
            }
        }

        return addresses
    }

    private func endpointIsOnSamePrivateLANSubnet(
        _ endpoint: String,
        localLANAddresses: [String]
    ) -> Bool {
        guard let endpointURL = URL(string: endpoint),
              let endpointHost = endpointURL.host,
              let endpointOctets = ipv4Octets(from: endpointHost)
        else {
            return false
        }

        return localLANAddresses.contains { localAddress in
            guard let localOctets = ipv4Octets(from: localAddress) else {
                return false
            }

            return endpointOctets.0 == localOctets.0
                && endpointOctets.1 == localOctets.1
                && endpointOctets.2 == localOctets.2
        }
    }

    private func ipv4Octets(from address: String) -> (Int, Int, Int, Int)? {
        let components = address.split(separator: ".")
        guard components.count == 4,
              let octet0 = Int(components[0]),
              let octet1 = Int(components[1]),
              let octet2 = Int(components[2]),
              let octet3 = Int(components[3]),
              (0...255).contains(octet0),
              (0...255).contains(octet1),
              (0...255).contains(octet2),
              (0...255).contains(octet3)
        else {
            return nil
        }

        return (octet0, octet1, octet2, octet3)
    }

    private func isPrivateLANIPv4Address(_ address: String) -> Bool {
        let components = address.split(separator: ".")
        guard components.count == 4,
              let firstOctet = Int(components[0]),
              let secondOctet = Int(components[1])
        else {
            return false
        }

        if firstOctet == 10 || firstOctet == 192 && secondOctet == 168 {
            return true
        }

        if firstOctet == 172,
           (16...31).contains(secondOctet) {
            return true
        }

        return false
    }
}

private func nowUnixTimeMilliseconds() -> UInt64 {
    UInt64(Date().timeIntervalSince1970 * 1_000)
}

private struct SharedCompanionBackendRecord: Codable, Equatable {
    let id: String
    let displayName: String
    let endpoint: String
    let lanEndpoint: String?
    let tailscaleEndpoint: String?
    let backendVersion: String?
    let minimumSupportedClientVersion: String?
    let sessionToken: String
    let updatedAtUnixMs: UInt64
}
