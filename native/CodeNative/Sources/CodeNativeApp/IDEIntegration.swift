import Foundation

#if os(macOS)
import AppKit

protocol IDEApplicationResolving {
    func urlForApplication(bundleIdentifier: String) -> URL?
    func urlForApplication(named name: String) -> URL?
}

struct LiveIDEApplicationResolver: IDEApplicationResolving {
    static let shared = LiveIDEApplicationResolver()

    func urlForApplication(bundleIdentifier: String) -> URL? {
        NSWorkspace.shared.urlForApplication(withBundleIdentifier: bundleIdentifier)
    }

    func urlForApplication(named name: String) -> URL? {
        let baseName = name.hasSuffix(".app") ? name : "\(name).app"
        let homeApplications = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Applications", isDirectory: true)
        let roots: [URL] = [
            URL(fileURLWithPath: "/Applications", isDirectory: true),
            homeApplications,
            URL(fileURLWithPath: "/System/Applications", isDirectory: true),
        ]

        for root in roots {
            let candidate = root.appendingPathComponent(baseName, isDirectory: true)
            if FileManager.default.fileExists(atPath: candidate.path) {
                return candidate
            }
        }

        return nil
    }
}
#endif

enum SessionIDEPreferences {
    static func decode(_ raw: String) -> [String: String] {
        guard let data = raw.data(using: .utf8),
              let decoded = try? JSONDecoder().decode([String: String].self, from: data)
        else {
            return [:]
        }

        return decoded
    }

    static func encode(_ map: [String: String]) -> String {
        guard let data = try? JSONEncoder().encode(map),
              let encoded = String(data: data, encoding: .utf8)
        else {
            return "{}"
        }

        return encoded
    }

    static func selectedIDE(
        for sessionID: UUID,
        rawMap: String,
        rawDefaultIDE: String,
        available: [SessionIDESelection]
    ) -> SessionIDESelection {
        let map = decode(rawMap)
        let availableSet = Set(available)
        guard let raw = map[sessionID.uuidString],
              let ide = SessionIDESelection(rawValue: raw),
              availableSet.contains(ide)
        else {
            return selectedDefaultIDE(rawDefaultIDE: rawDefaultIDE, available: available)
        }

        return ide
    }

    static func selectedDefaultIDE(
        rawDefaultIDE: String,
        available: [SessionIDESelection]
    ) -> SessionIDESelection {
        let availableSet = Set(available)
        guard let ide = SessionIDESelection(rawValue: rawDefaultIDE),
              availableSet.contains(ide)
        else {
            return .systemDefault
        }

        return ide
    }

    static func normalizedDefaultIDE(
        rawDefaultIDE: String,
        available: [SessionIDESelection]
    ) -> String {
        selectedDefaultIDE(rawDefaultIDE: rawDefaultIDE, available: available).rawValue
    }

    static func storing(
        ide: SessionIDESelection,
        for sessionID: UUID,
        rawMap: String
    ) -> String {
        var map = decode(rawMap)
        if ide == .systemDefault {
            map.removeValue(forKey: sessionID.uuidString)
        } else {
            map[sessionID.uuidString] = ide.rawValue
        }

        return encode(map)
    }

    static func pruned(
        rawMap: String,
        validSessionIDs: Set<UUID>,
        available: [SessionIDESelection]
    ) -> String {
        let availableSet = Set(available)
        let map = decode(rawMap)

        let pruned = map.filter { key, value in
            guard let sessionID = UUID(uuidString: key),
                  validSessionIDs.contains(sessionID),
                  let ide = SessionIDESelection(rawValue: value),
                  availableSet.contains(ide)
            else {
                return false
            }

            return ide != .systemDefault
        }

        return encode(pruned)
    }
}

#if os(macOS)
enum IDEAvailability {
    static func availableSelections(
        using resolver: IDEApplicationResolving = LiveIDEApplicationResolver.shared
    ) -> [SessionIDESelection] {
        SessionIDESelection.allCases.filter { selection in
            selection.isInstalled(using: resolver)
        }
    }
}
#endif
