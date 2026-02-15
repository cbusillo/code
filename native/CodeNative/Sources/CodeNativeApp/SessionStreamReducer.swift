import Foundation

enum SessionStreamReducer {
    // Keep the full local stream so transcript scrolling can always reach the
    // beginning of the attached session.
    private static let maxItems: Int? = nil

    private static func normalizedBySequence(
        _ items: [SessionStreamItem],
        maxItems: Int?
    ) -> [SessionStreamItem] {
        var seen: Set<UInt64> = []
        var ordered: [SessionStreamItem] = []
        ordered.reserveCapacity(items.count)

        for item in items.sorted(by: { $0.seq < $1.seq }) {
            if seen.insert(item.seq).inserted {
                ordered.append(item)
            }
        }

        guard let maxItems else {
            return ordered
        }

        return Array(ordered.suffix(maxItems))
    }

    static func shouldAcceptSessionAttached(
        selectedSessionID: UUID?,
        expectedSessionID: UUID?,
        attachedSessionID: UUID
    ) -> Bool {
        if let expectedSessionID,
           expectedSessionID != attachedSessionID {
            return false
        }

        guard let selectedSessionID else {
            return false
        }

        return selectedSessionID == attachedSessionID
    }

    static func mergeReplayItems(
        existing: [SessionStreamItem],
        incoming: [SessionStreamItem],
        fromSeq: UInt64,
        maxItems: Int? = SessionStreamReducer.maxItems
    ) -> [SessionStreamItem] {
        let normalizedExisting = normalizedBySequence(existing, maxItems: maxItems)

        if fromSeq == 0 {
            return normalizedBySequence(incoming, maxItems: maxItems)
        }

        guard let highestSeenSeq = normalizedExisting.last?.seq else {
            return normalizedBySequence(incoming, maxItems: maxItems)
        }

        var merged = normalizedExisting
        var existingSeq = Set(normalizedExisting.map(\.seq))

        for item in incoming.sorted(by: { $0.seq < $1.seq })
        where item.seq > highestSeenSeq && !existingSeq.contains(item.seq)
        {
            merged.append(item)
            existingSeq.insert(item.seq)
        }

        guard let maxItems else {
            return merged
        }

        return Array(merged.suffix(maxItems))
    }

    static func appendLiveItem(
        items: [SessionStreamItem],
        newItem: SessionStreamItem,
        maxItems: Int? = SessionStreamReducer.maxItems
    ) -> [SessionStreamItem] {
        let normalized = normalizedBySequence(items, maxItems: maxItems)

        if let lastSeq = normalized.last?.seq,
           newItem.seq <= lastSeq {
            return normalized
        }

        var next = normalized
        next.append(newItem)

        if let maxItems,
           next.count > maxItems {
            next.removeFirst(next.count - maxItems)
        }
        return next
    }
}
