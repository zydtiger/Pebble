import Foundation

struct CheckFailure: Error, CustomStringConvertible { let description: String }
func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw CheckFailure(description: message) }
}

// Synthetic delayed maintenance endpoint for the native Retry cache regression.
final class MemoryMaintenanceFixture {
    let process = Process()
    let url: String
    init(finalMarker: URL? = nil) throws {
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        process.arguments = ["-u", "-c", #"""
import json, time, sys
from pathlib import Path
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        data = json.loads(body['messages'][-1]['content'])
        if 'maintenance' in data:
            time.sleep(1.2)
            call = {'id':'maintenance-call','type':'function','function':{'name':'update_memory','arguments':json.dumps({'content':data['current_memory']})}}
            events = [{'choices':[{'index':0,'delta':{'tool_calls':[dict(call,index=0)]},'finish_reason':None}]}, {'choices':[{'index':0,'delta':{},'finish_reason':'tool_calls'}]}]
            encoded = (''.join('data: '+json.dumps(event)+'\n\n' for event in events)+'data: [DONE]\n\n').encode()
            status, kind = 200, 'text/event-stream'
        elif sys.argv[1] and body.get('tool_choice') != 'none':
            call = {'id':'saved-before-stop','type':'function','function':{'name':'update_memory','arguments':json.dumps({'content':'Saved before native room Stop.'})}}
            events = [{'choices':[{'index':0,'delta':{'tool_calls':[dict(call,index=0)]},'finish_reason':None}]}, {'choices':[{'index':0,'delta':{},'finish_reason':'tool_calls'}]}]
            encoded = (''.join('data: '+json.dumps(event)+'\n\n' for event in events)+'data: [DONE]\n\n').encode()
            status, kind = 200, 'text/event-stream'
        elif sys.argv[1]:
            Path(sys.argv[1]).touch()
            time.sleep(1.2)
            text = json.dumps({'reply':'late native final','state':{'affinity_user':99,'mood':'calm'}})
            events = [{'choices':[{'index':0,'delta':{'content':text},'finish_reason':None}]}, {'choices':[{'index':0,'delta':{},'finish_reason':'stop'}]}]
            encoded = (''.join('data: '+json.dumps(event)+'\n\n' for event in events)+'data: [DONE]\n\n').encode()
            status, kind = 200, 'text/event-stream'
        else:
            encoded, status, kind = b'{"error":"synthetic chat failure"}', 500, 'application/json'
        try:
            self.send_response(status); self.send_header('Content-Type', kind)
            self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError): pass
server = ThreadingHTTPServer(('127.0.0.1',0), Handler)
print(server.server_port, flush=True)
server.serve_forever()
"""#, finalMarker?.path ?? ""]
        process.standardOutput = output
        try process.run()
        var line = Data()
        while let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty {
            if byte[0] == 10 { break }; line.append(byte)
        }
        guard let port = String(data: line, encoding: .utf8), Int(port) != nil else {
            process.terminate(); process.waitUntilExit(); throw CheckFailure(description: "Maintenance fixture did not start")
        }
        url = "http://127.0.0.1:\(port)/v1"
    }
    deinit { if process.isRunning { process.terminate() }; process.waitUntilExit() }
}

@main
struct BridgeChecks {
    @MainActor
    static func main() async throws {
        let root = ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"].map { URL(fileURLWithPath: $0) } ?? FileManager.default.temporaryDirectory.appendingPathComponent("pebble-bridge-\(UUID())")
        defer { if ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"] == nil { try? FileManager.default.removeItem(at: root) } }
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
        let concurrentRoot = root.appendingPathComponent("concurrent")
        let maintainedPersona = try await exerciseConcurrentRooms(concurrentRoot)
        try await Task.sleep(for: .milliseconds(100))
        do {
            let restored = ChatModel(dataRoot: concurrentRoot)
            await restored.load()
            restored.select(maintainedPersona)
            for _ in 0..<100 where restored.loading { try await Task.sleep(for: .milliseconds(10)) }
            try require(restored.markers.count == 1, "Durable memory marker was not restored through FFI")
            try require(restored.maintenanceDetail?.contains("no chat model") == true, "Persisted maintenance status was not restored through FFI")
            restored.closeChat()
        }
        try await exerciseDailySettingsAndRetry(root.appendingPathComponent("daily"))
        try await exerciseMaintenanceRetry(root.appendingPathComponent("maintenance-retry"))
        try await Task.sleep(for: .milliseconds(100))
        do {
            let restored = try PebbleBackend(dataRoot: root.appendingPathComponent("maintenance-retry").path)
            let status = try restored.maintenanceStatus(personaId: restored.workspace().personas[0].id)
            try require(status?.memoryReviewed == true && status?.memorySaved == false, "FFI restart lost completed unchanged review status")
        }
        let stoppedRoot = root.appendingPathComponent("saved-memory-stop")
        let stoppedPersona = try await exerciseSavedMemoryStop(stoppedRoot)
        try await Task.sleep(for: .milliseconds(100))
        do {
            let restored = try PebbleBackend(dataRoot: stoppedRoot.path)
            let reply = try restored.messages(personaId: stoppedPersona).last
            try require(reply?.status == .cancelled && reply?.failureDetail.contains("Memory was saved") == true,
                        "Restart lost the cancelled reply's committed-memory explanation")
        }
        try await exerciseGroups(root.appendingPathComponent("groups"))
        let retryModel = ChatModel(dataRoot: root.appendingPathComponent("group-retry"))
        await retryModel.load()
        try await GroupRetryChecks.run(retryModel, retryAvailable: { retryModel.retryReply($0) }, retryBusy: { _ in })
        let classifierModel = ChatModel(dataRoot: root.appendingPathComponent("classifier"))
        await classifierModel.load()
        try await ClassifierChecks.run(classifierModel)
        let membershipModel = ChatModel(dataRoot: root.appendingPathComponent("membership"))
        await membershipModel.load()
        try await MembershipChecks.run(membershipModel)
        try await ReplyNotificationChecks.run(root.appendingPathComponent("reply-notifications"))
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
        model.sendMessage()
        for _ in 0..<100 where model.bubbles.count < 6 { try await Task.sleep(for: .milliseconds(10)) }
        model.stopReply()
        for _ in 0..<300 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.last?.status == .cancelled && model.bubbles.last?.canRetry == true, "Group cancellation/retry metadata failed: bubbles=\(model.bubbles.map { "\($0.status):\($0.canRetry)" }), error=\(model.error ?? "none"), replying=\(model.isReplying)")
        model.retryReply(model.bubbles.last!.id)
        for _ in 0..<400 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.count == 7 && model.bubbles.last?.status == .complete && model.bubbles.last?.senderID == a, "Group retry duplicated user or used wrong speaker/destination")
        model.closeChat()
    }

    @MainActor
    static func exerciseMaintenanceRetry(_ root: URL) async throws {
        let fixture = try MemoryMaintenanceFixture()
        let model = ChatModel(dataRoot: root)
        await model.load()
        let a = model.selected!.id
        let created = await model.createCharacter("Other")
        try require(created, "Could not create unrelated maintenance persona")
        try await ClassifierChecks.wait(model, until: { !model.loading })
        let b = model.selected!.id
        model.sendMessage("Cancelled unrelated reply")
        model.stopReply()
        try await ClassifierChecks.wait(model, until: { !model.hasActiveReplies })
        let other = model.bubbles.last!
        try require(model.canRetry(other), "Unrelated cancelled reply was not retryable")
        model.select(a)
        try await ClassifierChecks.wait(model, until: { !model.loading })
        let settings = try await model.readSettings(a)
        var chat = settings.chat
        chat.enabled = true; chat.contextLen = 65536; chat.maxOutputTokens = 4096
        chat.model = "synthetic-maintenance"; chat.baseUrl = fixture.url; chat.api = "chat_completions"
        try await model.saveSettings(a, update: SettingsUpdateRecord(chat: chat, credential: .clear, persona: settings.persona))
        model.sendMessage("Retryable failed input")
        try await ClassifierChecks.wait(model, until: { !model.hasActiveReplies })
        let stale = model.bubbles.last!
        try require(stale.canRetry && model.canRetry(stale), "Failed reply was not initially retryable")
        let operation = Task { await model.startFreshContext() }
        try await ClassifierChecks.wait(model, until: { model.isMaintaining })
        try require(stale.canRetry && !model.canRetry(stale), "Running maintenance did not hide stale cached Retry")
        model.select(b)
        try await ClassifierChecks.wait(model, until: { !model.loading })
        try require(model.canRetry(other), "Another persona's maintenance hid unrelated Retry")
        model.select(a)
        try await ClassifierChecks.wait(model, until: { !model.loading })
        model.cancelMaintenance()
        let completed = await operation.value
        try require(!completed && !model.isMaintaining && model.canRetry(model.bubbles.last!), "Retry did not return after maintenance terminated")
        let fresh = await model.startFreshContext()
        try require(fresh, "Unchanged native maintenance did not complete")
        let personaConfig = URL(fileURLWithPath: model.selected!.directory).appendingPathComponent("config.toml")
        let authored = try String(contentsOf: personaConfig, encoding: .utf8)
        try authored.replacingOccurrences(of: "synthetic-maintenance", with: "changed-maintenance")
            .write(to: personaConfig, atomically: true, encoding: .utf8)
        await model.load()
        let changed = await model.startFreshContext()
        try require(!changed && model.error?.hasPrefix("Couldn't start a fresh context.") == true && model.error?.contains("Couldn't start the reply") == false,
                    "Fresh-context start error used reply wording")
        model.closeChat()
        print("Native maintenance passed: cached Retry isolation, completed unchanged review, and accurate fresh-context start error.")
    }

    @MainActor
    static func exerciseSavedMemoryStop(_ root: URL) async throws -> String {
        let marker = root.appendingPathComponent("final-request-ready")
        let fixture = try MemoryMaintenanceFixture(finalMarker: marker)
        let model = ChatModel(dataRoot: root)
        await model.load()
        let id = model.selected!.id
        let settings = try await model.readSettings(id)
        var chat = settings.chat
        chat.enabled = true; chat.contextLen = 65536; chat.maxOutputTokens = 4096
        chat.model = "synthetic-memory-stop"; chat.baseUrl = fixture.url; chat.api = "chat_completions"
        try await model.saveSettings(id, update: SettingsUpdateRecord(chat: chat, credential: .clear, persona: settings.persona))
        model.sendMessage("Remember before Stop")
        try await ClassifierChecks.wait(model, until: { FileManager.default.fileExists(atPath: marker.path) })
        model.stopReply()
        try await ClassifierChecks.wait(model, until: { !model.hasActiveReplies })
        let reply = model.bubbles.last!
        try require(reply.status == .cancelled && reply.text.isEmpty && reply.failureDetail.contains("Memory was saved"),
                    "Cancelled bubble omitted its durable committed-memory explanation")
        await model.load()
        try require(model.bubbles.last?.failureDetail == reply.failureDetail,
                    "Reload lost the cancelled bubble's committed-memory explanation")
        model.closeChat()
        print("Native room Stop passed: cancelled bubble retains committed-memory detail through FFI and reload.")
        return id
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
        let update = SettingsUpdateRecord(chat: settings.chat, credential: .keep, persona: persona)
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
        connection.enabled = true; connection.contextLen = 65536; connection.maxOutputTokens = 4096; connection.model = "synthetic-model"
        connection.baseUrl = "http://127.0.0.1:9/v1"; connection.api = "responses"
        connection.responsesSchemaFormat = "response_format"
        try await model.saveSettings(nil, update: SettingsUpdateRecord(chat: connection, credential: .keep, persona: nil))
        let dialect = try await model.readSettings(nil)
        try require(dialect.chat.contextLen == 65536 && dialect.chat.maxOutputTokens == 4096, "Chat limits did not cross native settings bridge")
        try require(dialect.chat.responsesSchemaFormat == "response_format", "Responses dialect did not cross native settings bridge")
        model.closeChat()
    }

    @MainActor
    static func exerciseConcurrentRooms(_ root: URL) async throws -> String {
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
        try require(model.markers.count == 1 && model.markers[0].personaId == mira, "Persona memory marker projection was missing")
        try require(model.maintenanceDetail?.contains("no chat model") == true, "Demo maintenance pretended a model memory save")
        try require(model.bubbles.map(\.id) == saved, "Fresh context deleted visible messages")
        try require(model.selected?.contextNotice.contains("Fresh model context") == true, "Fresh context notice was missing")
        try require(model.draft == "Mira's unsent draft", "Fresh context cleared the unsent draft")
        model.sendMessage("New context input")
        for _ in 0..<300 where model.hasActiveReplies { try await Task.sleep(for: .milliseconds(10)) }
        try require(model.bubbles.last?.status == .complete && !model.bubbles.last!.text.contains("Mira first private input"), "Fresh context reused earlier model context: loading=\(model.loading), replying=\(model.isReplying), error=\(model.error ?? "none"), last=\(model.bubbles.last?.text ?? "none")")
        model.closeChat()
        return mira
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
