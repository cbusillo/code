import Foundation

enum SessionStreamReducer {
    // Keep the full local stream so transcript scrolling can always reach the
    // beginning of the attached session.
    private static let maxItems: Int? = nil

    private static func trimToMaxItems(_ items: [SessionStreamItem], maxItems: Int?) -> [SessionStreamItem] {
        guard let maxItems,
              items.count > maxItems
        else {
            return items
        }

        return Array(items.suffix(maxItems))
    }

    private static func normalizeUnordered(
        _ items: [SessionStreamItem],
        maxItems: Int?
    ) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return []
        }

        let sorted = items.sorted(by: { $0.seq < $1.seq })
        var deduped: [SessionStreamItem] = []
        deduped.reserveCapacity(sorted.count)

        var lastSeq: UInt64?
        for item in sorted {
            if item.seq == lastSeq {
                continue
            }
            deduped.append(item)
            lastSeq = item.seq
        }

        return trimToMaxItems(deduped, maxItems: maxItems)
    }

    private static func ensureNormalized(
        _ items: [SessionStreamItem],
        maxItems: Int?
    ) -> [SessionStreamItem] {
        guard !items.isEmpty else {
            return []
        }

        var previousSeq = items[0].seq
        for item in items.dropFirst() {
            if item.seq <= previousSeq {
                return normalizeUnordered(items, maxItems: maxItems)
            }
            previousSeq = item.seq
        }

        return trimToMaxItems(items, maxItems: maxItems)
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
        if fromSeq == 0 {
            return normalizeUnordered(incoming, maxItems: maxItems)
        }

        let normalizedExisting = ensureNormalized(existing, maxItems: maxItems)

        guard let highestSeenSeq = normalizedExisting.last?.seq else {
            return normalizeUnordered(incoming, maxItems: maxItems)
        }

        var merged = normalizedExisting
        var lastAppendedSeq = highestSeenSeq

        for item in incoming.sorted(by: { $0.seq < $1.seq })
        where item.seq > highestSeenSeq && item.seq != lastAppendedSeq
        {
            merged.append(item)
            lastAppendedSeq = item.seq
        }

        return trimToMaxItems(merged, maxItems: maxItems)
    }

    static func appendLiveItem(
        items: [SessionStreamItem],
        newItem: SessionStreamItem,
        maxItems: Int? = SessionStreamReducer.maxItems
    ) -> [SessionStreamItem] {
        let normalized = ensureNormalized(items, maxItems: maxItems)

        if let lastSeq = normalized.last?.seq,
           newItem.seq <= lastSeq {
            return normalized
        }

        var next = normalized
        next.append(newItem)

        return trimToMaxItems(next, maxItems: maxItems)
    }

    static func mergeOlderHistoryPage(
        existing: [SessionStreamItem],
        incoming: [SessionStreamItem],
        beforeSeq: UInt64,
        maxItems: Int? = SessionStreamReducer.maxItems
    ) -> [SessionStreamItem] {
        let normalizedExisting = ensureNormalized(existing, maxItems: maxItems)

        var olderItems = incoming
            .filter { $0.seq < beforeSeq }
            .sorted(by: { $0.seq < $1.seq })

        olderItems = ensureNormalized(olderItems, maxItems: nil)

        if let existingFirstSeq = normalizedExisting.first?.seq {
            olderItems.removeAll { $0.seq >= existingFirstSeq }
        }

        if olderItems.isEmpty {
            return normalizedExisting
        }

        return trimToMaxItems(olderItems + normalizedExisting, maxItems: maxItems)
    }
}
