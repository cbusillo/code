import SwiftUI

struct ContentView: View {
    @ObservedObject var store: SessionMirrorStore

    var body: some View {
        NavigationSplitView {
            sidebar
        } detail: {
            transcript
        }
        .navigationSplitViewStyle(.balanced)
        .task {
            if store.connectionState == .disconnected {
                await store.connect()
            }
        }
    }

    private var sidebar: some View {
        VStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 6) {
                Text("Session Endpoint")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                TextField("ws://127.0.0.1:4317/ws", text: $store.endpoint)
                    .textFieldStyle(.roundedBorder)
            }

            HStack(spacing: 8) {
                Button("Connect") {
                    Task {
                        await store.connect()
                    }
                }
                .disabled(store.connectionState != .disconnected)

                Button("Disconnect") {
                    store.disconnect()
                }
                .disabled(store.connectionState == .disconnected)

                Button("Refresh") {
                    Task {
                        await store.refreshSessions()
                    }
                }
                .disabled(store.connectionState != .connected)
            }
            .buttonStyle(.borderedProminent)

            HStack {
                Text("Status")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                Spacer()
                Text(store.statusLine)
                    .font(.caption)
            }

            if let lastError = store.lastError {
                Text(lastError)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            List(selection: $store.selectedSessionID) {
                Section("Sessions") {
                    ForEach(store.sessions) { session in
                        SessionRow(session: session)
                            .tag(session.id)
                    }
                }
            }
        }
        .padding(12)
        .navigationTitle("Code Native")
    }

    private var transcript: some View {
        VStack(alignment: .leading, spacing: 12) {
            if let session = store.selectedSession {
                VStack(alignment: .leading, spacing: 4) {
                    Text("Session \(session.id.uuidString.prefix(8))")
                        .font(.title3.weight(.semibold))
                    Text("\(session.model) · \(session.cwd)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 10) {
                        ForEach(store.selectedSessionItems) { item in
                            TranscriptCard(item: item)
                        }
                    }
                    .padding(.vertical, 4)
                }
            } else {
                ContentUnavailableView(
                    "No Session Selected",
                    systemImage: "rectangle.stack",
                    description: Text("Connect and select a session to view its live transcript.")
                )
            }
        }
        .padding(16)
    }
}

private struct SessionRow: View {
    let session: SessionSummary

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(session.model)
                .font(.subheadline.weight(.semibold))
            Text(session.cwd)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
            Text(session.id.uuidString)
                .font(.caption2.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

private struct TranscriptCard: View {
    let item: SessionStreamItem

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                Text(item.title)
                    .font(.caption.weight(.semibold))
                    .textCase(.uppercase)
                    .foregroundStyle(.secondary)
                Spacer()
                Text("#\(item.seq)")
                    .font(.caption.monospaced())
                    .foregroundStyle(.secondary)
            }

            Text(item.body)
                .font(.body.monospaced())
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        .padding(12)
        .background(Color(nsColor: .textBackgroundColor))
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(Color.secondary.opacity(0.2), lineWidth: 1)
        )
    }
}
