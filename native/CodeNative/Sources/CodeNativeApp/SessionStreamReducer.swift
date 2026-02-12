import Foundation

enum SessionStreamReducer {
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
        maxItems: Int = 2_000
    ) -> [SessionStreamItem] {
        if fromSeq == 0 {
            return incoming.suffix(maxItems)
        }

        var merged = existing
        var existingSeq = Set(existing.map(\.seq))

        for item in incoming where !existingSeq.contains(item.seq) {
            merged.append(item)
            existingSeq.insert(item.seq)
        }

        if merged.count > maxItems {
            merged.removeFirst(merged.count - maxItems)
        }

        return merged
    }

    static func appendLiveItem(
        items: [SessionStreamItem],
        newItem: SessionStreamItem,
        maxItems: Int = 2_000
    ) -> [SessionStreamItem] {
        if let lastSeq = items.last?.seq,
           newItem.seq <= lastSeq {
            return items
        }

        var next = items
        next.append(newItem)
        if next.count > maxItems {
            next.removeFirst(next.count - maxItems)
        }
        return next
    }
}
