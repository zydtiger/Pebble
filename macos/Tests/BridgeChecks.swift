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
        try await exerciseDailySettingsAndRetry(root.appendingPathComponent("daily"))
        try await exerciseGroups(root.appendingPathComponent("groups"))
        let retryModel = ChatModel(dataRoot: root.appendingPathComponent("group-retry"))
        await retryModel.load()
        try await GroupRetryChecks.run(retryModel, retryAvailable: { retryModel.retryReply($0) }, retryBusy: { _ in })
        let classifierModel = ChatModel(dataRoot: root.appendingPathComponent("classifier"))
        await classifierModel.load()
        try await ClassifierChecks.run(classifierModel)
        print("Swift 6 FFI checks passed: durable IDs/history, native records, validated Unicode replies, cancellation, listener release, UI restart and close lifecycle.")
    }

    @MainActor
    static func exerciseGroups(_ root: URL) async throws {
        let model = ChatModel(dataRoot: root)
        await model.load()
        var classifier = try await model.readClassifierSettings(nil)
        classifier.connection.enabled = false
        try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
        let a = model.selected!.id
        let created = await model.createCharacter("Kai"); try require(created, "Could not create group member")
        for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(10)) }
        let b = model.selected!.id
        let grouped = await model.createGroup("Native group", members: [a, b]); try require(grouped, "Could not create native group")
        for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(10)) }
        let group = model.selectedConversation!
        try require(group.kind == "group" && model.selected == nil && group.members.count == 2, "Room identity/member bridge failed")
        model.draft = "Everyone can listen"
        model.sendMessage()
        for _ in 0..<100 where model.isReplying { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 1 && model.bubbles[0].isUser && model.draft.isEmpty, "Unmentioned input was not saved once without reply")
        model.mentionedIDs = [a, b]
        model.draft = "Chosen mentions remain explicit after editing"
        model.sendMessage()
        model.select(a)
        model.draft = "Private room draft"
        for _ in 0..<400 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        model.select(group.id)
        for _ in 0..<100 where model.loading { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 4 && Set(model.bubbles.filter { !$0.isUser }.map(\.senderID)) == Set([a, b]), "Shared listener ended early or speaker/destination routing changed")
        try require(model.mentionedIDs.isEmpty && model.draft.isEmpty, "Accepted mention chips/draft not cleared")
        let renamed = await model.renameGroup("Renamed native group"); try require(renamed, "Native group rename failed")
        try require(model.selectedConversation?.id == group.id && model.selectedConversation?.title == "Renamed native group", "Rename changed room identity")
        model.mentionedIDs = [a]
        model.draft = "Keep chosen target while editing text"
        model.select(a)
        try require(model.draft == "Private room draft", "Group stream overwrote private draft")
        model.select(group.id)
        try require(model.mentionedIDs == [a] && model.draft == "Keep chosen target while editing text", "Room switching lost mention selection")
        model.sendMessage(); model.stopReply()
        for _ in 0..<300 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.last?.status == .cancelled && model.bubbles.last?.canRetry == true, "Group cancellation/retry metadata failed")
        model.retryReply(model.bubbles.last!.id)
        for _ in 0..<400 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 7 && model.bubbles.last?.status == .complete && model.bubbles.last?.senderID == a, "Group retry duplicated user or used wrong speaker/destination")
        model.closeChat()
    }

    @MainActor
    static func exerciseDailySettingsAndRetry(_ root: URL) async throws {
        let model = ChatModel(dataRoot: root)
        await model.load()
        let id = model.selected!.id
        let settings = try await model.readSettings(id)
        var persona = settings.persona!
        persona.displayName = "Edited name"
        persona.soul = "Edited SOUL"
        let update = SettingsUpdateRecord(chat: settings.chat, credential: .keep, contextWindowTokens: settings.contextWindowTokens, maxOutputTokens: settings.maxOutputTokens, persona: persona)
        try await model.saveSettings(id, update: update)
        try require(model.selected?.displayName == "Edited name", "Settings did not refresh the native persona")
        model.sendMessage("Retry one user message")
        model.stopReply()
        for _ in 0..<200 where model.isReplying { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 2 && model.bubbles[1].canRetry, "Cancelled latest reply did not expose Retry")
        let userID = model.bubbles[0].id
        model.draft = "Preserve this draft"
        model.retryReply(model.bubbles[1].id)
        for _ in 0..<300 where model.isReplying { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 3 && model.bubbles[0].id == userID && model.bubbles[2].status == .complete, "Retry duplicated user or failed routing")
        try require(model.bubbles[1].status == .cancelled && model.draft == "Preserve this draft", "Retry replaced partial attempt or draft")
        let before = try await model.readResponseSettings(id)
        let savedMessages = model.bubbles.map(\.id)
        let initial = "{\"affinity_user\":7,\"mood\":\"quiet\"}"
        try await model.saveResponseSettings(id, update: ResponseSettingsUpdateRecord(expected: before, schemaJson: before.schemaJson, initialStateJson: initial, action: .initialDefaultsOnly))
        let defaults = try await model.readResponseSettings(id)
        try require(defaults.currentStateJson == before.currentStateJson && defaults.sessionId == before.sessionId, "Initial-default save reset state/context")
        try await model.saveResponseSettings(id, update: ResponseSettingsUpdateRecord(expected: defaults, schemaJson: defaults.schemaJson, initialStateJson: defaults.initialStateJson, action: .freshContextResetState))
        let reset = try await model.readResponseSettings(id)
        try require(reset.sessionId != before.sessionId && reset.currentStateJson.contains("quiet"), "Explicit reset did not establish state/context boundary")
        try require(model.bubbles.map(\.id) == savedMessages, "State reset changed native transcript presentation")
        do {
            try await model.saveResponseSettings(id, update: ResponseSettingsUpdateRecord(expected: before, schemaJson: before.schemaJson, initialStateJson: before.initialStateJson, action: .freshContextKeepState))
            throw CheckFailure(description: "Stale state controls were accepted")
        } catch BackendError.Settings { /* Recoverable stale-panel error crosses the real bridge. */ }
        let app = try await model.readSettings(nil)
        var connection = app.chat
        connection.enabled = true; connection.model = "synthetic-model"
        connection.baseUrl = "http://127.0.0.1:9/v1"; connection.api = "responses"
        connection.responsesSchemaFormat = "response_format"
        try await model.saveSettings(nil, update: SettingsUpdateRecord(chat: connection, credential: .keep, contextWindowTokens: app.contextWindowTokens, maxOutputTokens: app.maxOutputTokens, persona: nil))
        let dialect = try await model.readSettings(nil)
        try require(dialect.chat.responsesSchemaFormat == "response_format", "Responses dialect did not cross native settings bridge")
        model.closeChat()
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
        try require(model.bubbles.last?.status == .complete && !model.bubbles.last!.text.contains("Mira first private input"), "Fresh context reused earlier model context: loading=\(model.loading), replying=\(model.isReplying), error=\(model.error ?? "none"), last=\(model.bubbles.last?.text ?? "none")")
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
            try require(update.kind != .textDelta, "Structured candidate leaked before validation")
            if update.kind == .completed {
                terminal = true
                text = update.messages[0].content
                try require(!text.contains("affinity_user") && !text.contains("\"reply\""), "Structured envelope leaked into the bubble")
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
