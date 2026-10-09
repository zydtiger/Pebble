import Foundation

struct CheckFailure: Error, CustomStringConvertible { let description: String }
func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw CheckFailure(description: message) }
}

@main
struct BridgeChecks {
    @MainActor
    static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pebble-bridge-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let (personaID, roomID, messageIDs) = try await exerciseBridge(root)
        try await Task.sleep(for: .milliseconds(100))
        do {
            let reopened = try PebbleBackend(dataRoot: root.path)
            let persona = try reopened.workspace().personas[0]
            let messages = try reopened.messages(personaId: personaID)
            try require(persona.id == personaID && persona.conversationId == roomID, "Identity changed on restart")
            try require(messages.map(\.id) == messageIDs, "Message IDs changed on restart")
            try require(messages[1].status == .complete && messages[3].status == .cancelled, "Terminal states were not saved")
        }
        let uiRoot = root.appendingPathComponent("ui")
        let savedIDs = try await exercisePresentation(uiRoot)
        try await Task.sleep(for: .milliseconds(100))
        let reopened = ChatModel(dataRoot: uiRoot)
        await reopened.load()
        try require(reopened.error == nil, "Could not reopen presentation: \(reopened.error ?? "")")
        try require(reopened.bubbles.map(\.id) == savedIDs, "UI did not restore the durable transcript")
        try require(reopened.bubbles.last?.status == .complete, "UI lost the completed reply")
        reopened.closeChat()
        try await exerciseConcurrentRooms(root.appendingPathComponent("concurrent"))
        print("Swift 6 FFI checks passed: durable IDs/history, native records, Unicode deltas, cancellation, listener release, UI restart and close lifecycle.")
    }

    @MainActor
    static func exerciseConcurrentRooms(_ root: URL) async throws {
        let model = ChatModel(dataRoot: root)
        await model.load()
        let mira = model.selected!.id
        model.draft = "Mira's unsent draft"
        let created = await model.createCharacter("Kai")
        try require(created, "Character creation failed")
        for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(10)) }
        let kai = model.selected!.id
        model.draft = "Kai's unsent draft"
        model.select(mira)
        try require(model.draft == "Mira's unsent draft", "Switch lost Mira draft")
        model.sendMessage("Mira first private input")
        model.select(kai)
        try require(model.draft == "Kai's unsent draft", "Switch lost Kai draft")
        model.sendMessage("Kai private input")
        try require(model.pendingCount(mira) == 1 && model.pendingCount(kai) == 1, "Personas did not remain independently active")
        model.select(mira)
        model.sendMessage("Mira followup")
        model.sendMessage("Cancel queued Mira input")
        for _ in 0..<100 where model.bubbles.count < 6 { try await Task.sleep(for: .milliseconds(5)) }
        try require(model.bubbles.count == 6 && model.bubbles.last?.status == .queued, "Queued bubble was not visible")
        let resetWhileQueued = await model.startFreshContext()
        try require(!resetWhileQueued, "Fresh context accepted outstanding queued work")
        model.cancelReply(model.bubbles.last!.id)
        model.select(kai)
        model.stopReply()
        for _ in 0..<300 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(!model.hasActiveReplies, "Concurrent rooms did not terminate")
        try require(model.bubbles.count == 2 && model.bubbles.last?.status == .cancelled, "Stop did not cancel selected Kai room")
        try require(model.bubbles.first?.text == "Kai private input", "Mira output leaked into Kai room")
        model.select(mira)
        try require(model.bubbles[1].status == .complete && model.bubbles[3].status == .complete,
                    "Stopping Kai cancelled Mira or lost queued followup")
        try require(model.bubbles[5].status == .cancelled, "Queued cancel was lost")
        try require(model.bubbles[3].text.contains("Mira first private input"), "Followup did not use predecessor history")
        try require(!model.bubbles.contains { $0.text.contains("Kai private input") }, "Kai output leaked into Mira room")
        try require(model.draft == "Mira's unsent draft", "Background stream cleared another draft")
        let saved = model.bubbles.map(\.id)
        let reset = await model.startFreshContext()
        try require(reset && model.error == nil, "Fresh context failed after terminal replies")
        try require(model.bubbles.map(\.id) == saved, "Fresh context deleted visible messages")
        try require(model.selected?.contextNotice.contains("Fresh model context") == true, "Fresh context notice was missing")
        try require(model.draft == "Mira's unsent draft", "Fresh context cleared the unsent draft")
        model.sendMessage("New context input")
        for _ in 0..<300 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.last?.status == .complete && !model.bubbles.last!.text.contains("Mira first private input"), "Fresh context reused earlier model context")
        model.closeChat()
    }

    static func exerciseBridge(_ root: URL) async throws -> (String, String, [String]) {
        let client = CoreClient(root: root.path)
        let first = try await client.load()
        try require(first.personas.count == 1, "Expected the starter character")
        let persona = first.personas[0]
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        var observer: StreamObserver? = StreamObserver(sink)
        weak let weakObserver = observer
        var handle: ChatHandle? = try await client.send(persona: persona.id, prompt: "你好", observer: observer!)
        let runID = handle!.id()
        let acceptedIDs = handle!.messages().map(\.id)
        observer = nil
        var sequence: UInt64 = 0
        var text = ""
        var terminal = false
        for await update in stream {
            try require(update.sequence == sequence, "Event order changed at the Swift boundary")
            try require(update.runId == runID && update.conversationId == persona.conversationId, "Wrong call or room")
            try require(update.messageId == acceptedIDs[1], "Delta changed message identity")
            sequence += 1
            if update.kind == .textDelta { text += update.text }
            if update.kind == .completed {
                terminal = true
                try require(update.messages[0].content == text, "Final durable content differs from streamed text")
            }
        }
        try require(terminal && text.contains("本地演示"), "Unicode demo stream did not complete")
        let saved = try await client.history(persona: persona.id)
        try require(saved.map(\.id) == acceptedIDs && saved[1].content == text, "History differs from visible output")
        handle = nil
        for _ in 0..<50 where weakObserver != nil { try await Task.sleep(for: .milliseconds(10)) }
        try require(weakObserver == nil, "Rust retained the Swift listener")

        let (cancelStream, cancelSink) = AsyncStream<ChatUpdate>.makeStream()
        let cancelled = try await client.send(persona: persona.id, prompt: "hello", observer: StreamObserver(cancelSink))
        cancelled.cancel()
        var cancelledTerminal = false
        for await update in cancelStream {
            if update.kind == .cancelled { cancelledTerminal = true }
            try require(update.kind != .completed, "Cancellation became completion")
        }
        try require(cancelledTerminal, "Rust did not acknowledge cancellation")
        do {
            let (unused, unusedSink) = AsyncStream<ChatUpdate>.makeStream()
            _ = unused
            _ = try await client.send(persona: "not-a-persona", prompt: "hello", observer: StreamObserver(unusedSink))
            throw CheckFailure(description: "Missing typed error")
        } catch BackendError.UnknownPersona { }
        return (persona.id, persona.conversationId, try await client.history(persona: persona.id).map(\.id))
    }

    @MainActor
    static func exercisePresentation(_ root: URL) async throws -> [String] {
        let model = ChatModel(dataRoot: root)
        await model.load()
        try require(model.selected != nil, "Presentation did not load a character")
        model.draft = "hello"
        model.sendMessage()
        model.stopReply()
        for _ in 0..<100 where model.isReplying { try await Task.sleep(for: .milliseconds(20)) }
        try require(!model.isReplying && model.bubbles.last?.status == .cancelled, "Early UI cancellation was lost")
        try require(model.draft.isEmpty, "Accepted draft was not cleared")
        model.sendMessage("another hello")
        model.closeChat()
        try await Task.sleep(for: .milliseconds(150))
        try require(!model.isReplying, "Closing left a reply running")
        await model.load()
        try require(model.bubbles.count == 4 && model.bubbles.last?.status == .cancelled, "Closing lost durable accepted messages")
        model.sendMessage("After reopening")
        for _ in 0..<150 where model.isReplying { try await Task.sleep(for: .milliseconds(20)) }
        try require(model.error == nil && model.bubbles.count == 6, "Could not continue saved history")
        try require(Set(model.bubbles.map(\.id)).count == 6, "UI duplicated a saved message")
        let ids = model.bubbles.map(\.id)
        model.closeChat()
        return ids
    }
}
