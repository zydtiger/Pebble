import Foundation

enum GroupRetryFailure: Error { case failed(String) }

enum GroupRetryChecks {
    @MainActor
    static func run(_ model: ChatModel,
                    retryAvailable: (String) async throws -> Void,
                    retryBusy: (String) async throws -> Void) async throws {
        var classifier = try await model.readClassifierSettings(nil)
        classifier.connection.enabled = false
        try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
        let a = model.selected!.id
        guard await model.createCharacter("Kai") else { throw GroupRetryFailure.failed("Could not create retry speaker") }
        try await wait { !model.loading }
        let b = model.selected!.id
        guard await model.createGroup("Retry across speakers", members: [a, b]) else { throw GroupRetryFailure.failed("Could not create retry group") }
        try await wait { !model.loading }
        let group = model.selectedConversation!.id

        // Real demo calls keep B's persona queue occupied while A cancels and retries.
        model.select(b)
        for index in 0..<8 { model.sendMessage("B private predecessor \(index)") }
        model.select(group)
        model.mentionedIDs = [a, b]
        model.sendMessage("Both characters may answer")
        try await wait { model.bubbles.contains { !$0.isUser && $0.senderID == a } }
        let firstA = model.bubbles.first { !$0.isUser && $0.senderID == a }!.id
        model.cancelReply(firstA)
        try await wait { model.bubbles.contains { !$0.isUser && $0.senderID == b } }
        let firstB = model.bubbles.first { !$0.isUser && $0.senderID == b }!.id
        try await wait { model.bubbles.first { $0.id == firstA }.map { model.canRetry($0) } == true }
        guard model.isReplying, pending(model, firstB), model.canRetry(model.bubbles.first { $0.id == firstA }!) else {
            throw GroupRetryFailure.failed("Idle A Retry hidden while unrelated B is pending")
        }
        try await retryAvailable(firstA)
        try await wait { model.bubbles.contains { !$0.isUser && $0.senderID == a && $0.id != firstA && $0.status == .complete } }
        guard model.bubbles.filter(\.isUser).count == 1, pending(model, firstB) else {
            throw GroupRetryFailure.failed("Retry duplicated user or cancelled/finished unrelated B unexpectedly")
        }

        // Ready-only scheduling correctly keeps a newer same-room required A behind B.
        // A separate room lets this independent A call run while the original B stays queued.
        guard await model.createGroup("Retry shared persona", members: [a]) else { throw GroupRetryFailure.failed("Could not create independent retry room") }
        try await wait { !model.loading }
        let retryRoom = model.selectedID!
        model.mentionedIDs = [a]
        model.sendMessage("Cancel A again before its private call")
        try await wait { model.bubbles.last?.senderID == a }
        let secondA = model.bubbles.last!.id
        model.cancelReply(secondA)
        try await wait { model.bubbles.first { $0.id == secondA }?.canRetry == true }
        model.select(a)
        model.sendMessage("A now has work in another room")
        model.select(retryRoom)
        guard let stale = model.bubbles.first(where: { $0.id == secondA }), stale.canRetry, !model.canRetry(stale) else {
            throw GroupRetryFailure.failed("Stale cached eligibility ignored A's other-room work")
        }
        try await retryBusy(secondA)
        let before = model.bubbles.count
        model.retryReply(secondA)
        try await Task.sleep(for: .milliseconds(30))
        guard model.bubbles.count == before, model.error == nil else { throw GroupRetryFailure.failed("Busy Retry was admitted") }
        try await wait { model.pendingCount(a) == 0 && model.bubbles.first(where: { $0.id == secondA })?.canRetry == false }
        guard model.pendingCount(group) > 0, !model.canRetry(model.bubbles.first { $0.id == secondA }!) else {
            throw GroupRetryFailure.failed("Old A Retry reappeared after its newer private call while B remained pending")
        }
        try await retryBusy(secondA)
        try await wait { !model.hasActiveReplies }
        let superseded = model.bubbles.first(where: { $0.id == secondA })?.canRetry == false
        model.select(group)
        guard model.bubbles.first(where: { $0.id == firstB })?.status == .complete,
              superseded else {
            throw GroupRetryFailure.failed("B was cancelled or old A eligibility survived its newer private call")
        }
        model.select(a)
        guard model.bubbles.last?.status == .complete else { throw GroupRetryFailure.failed("A private call did not finish") }
        model.closeChat()
        print("Group Retry passed: idle A can retry while B remains pending; real retry preserves B; shared A busy in another room hides stale eligibility.")
    }

    @MainActor
    private static func pending(_ model: ChatModel, _ id: String) -> Bool {
        model.bubbles.contains { $0.id == id && ($0.status == .queued || $0.status == .streaming) }
    }

    @MainActor
    private static func wait(_ ready: () -> Bool) async throws {
        for _ in 0..<1800 {
            if ready() { return }
            try await Task.sleep(for: .milliseconds(10))
        }
        throw GroupRetryFailure.failed("Timed out waiting for group Retry fixture")
    }
}
