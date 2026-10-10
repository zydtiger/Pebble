import Foundation

// Local synthetic endpoint: no user runtime or external service is used by native gates.
final class ClassifierFixture {
    let process = Process()
    let url: String
    init() throws {
        let output = Pipe()
        process.executableURL = URL(fileURLWithPath: "/usr/bin/python3")
        process.arguments = ["-u", "-c", #"""
import json, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        source = body.get('messages', body.get('input'))[1]['content']
        if isinstance(source, list): source = source[0]['text']
        data = json.loads(source)
        latest = data['recent_group_transcript'][-1]['text']
        time.sleep(1.2 if 'hold' in latest else 0.05)
        text = 'invalid boolean' if 'malformed' in latest else json.dumps({'respond': 'volunteer' in latest})
        response = {'choices': [{'finish_reason': 'stop', 'message': {'role': 'assistant', 'content': text}}]}
        encoded = json.dumps(response).encode()
        try:
            self.send_response(200); self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(encoded))); self.end_headers(); self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError): pass
server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
print(server.server_port, flush=True)
server.serve_forever()
"""#]
        process.standardOutput = output
        try process.run()
        var line = Data()
        while let byte = try output.fileHandleForReading.read(upToCount: 1), !byte.isEmpty {
            if byte[0] == 10 { break }; line.append(byte)
        }
        guard let port = String(data: line, encoding: .utf8), Int(port) != nil else {
            process.terminate(); process.waitUntilExit(); throw ClassifierCheckError.failed("Local classifier fixture did not start")
        }
        url = "http://127.0.0.1:\(port)/v1"
    }
    deinit { if process.isRunning { process.terminate() }; process.waitUntilExit() }
}

enum ClassifierCheckError: Error { case failed(String) }
@MainActor
enum ClassifierChecks {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        if !condition() { throw ClassifierCheckError.failed(message) }
    }
    static func wait(_ model: ChatModel, until condition: () -> Bool) async throws {
        for _ in 0..<600 { if condition() { return }; try await Task.sleep(for: .milliseconds(10)) }
        throw ClassifierCheckError.failed("Native classifier lifecycle timed out: \(model.error ?? "no chat error")")
    }
    static func checkPendingBoundaries(_ model: ChatModel, persona: String, room: String) async throws {
        let before = try await model.readResponseSettings(persona)
        model.select(persona)
        try await wait(model, until: { !model.loading })
        let fresh = await model.startFreshContext()
        try check(!fresh && model.error?.contains("participation checks") == true, "Pending classifier did not block native fresh context with an actionable error")
        for action: ResponseSettingsAction in [.initialDefaultsOnly, .freshContextKeepState, .freshContextResetState] {
            do {
                try await model.saveResponseSettings(persona, update: ResponseSettingsUpdateRecord(expected: before, schemaJson: before.schemaJson, initialStateJson: "{\"affinity_user\":7,\"mood\":\"quiet\"}", action: action))
                throw ClassifierCheckError.failed("Pending classifier allowed native response publication: \(action)")
            } catch BackendError.Busy { }
        }
        let after = try await model.readResponseSettings(persona)
        try check(after == before, "Rejected native response controls changed state/context/defaults")
        model.error = nil
        model.select(room)
        try await wait(model, until: { !model.loading })
        try check(model.participation.allSatisfy { $0.status == "pending" }, "Native boundary checks outlived the held classifier fixture")
        print("Native pending-classifier boundary checks passed: fresh/defaults/keep/reset Busy, unchanged snapshot, actionable participation error.")
    }
    static func run(_ model: ChatModel, phase: (String) async throws -> Void = { _ in }) async throws {
        let fixture = try ClassifierFixture()
        let a = model.selected!.id
        let created = await model.createCharacter("Kai")
        try check(created, "Could not create classifier member")
        try await wait(model, until: { !model.loading })
        let b = model.selected!.id
        for target in [nil, Optional(a)] {
            var settings = try await model.readClassifierSettings(target)
            settings.connection.enabled = true; settings.connection.model = "synthetic-participation"
            settings.connection.baseUrl = fixture.url; settings.connection.api = "chat_completions"
            settings.requestProfile = "qwen_non_thinking"
            try await model.saveClassifierSettings(target, update: settings, credential: .clear)
            for dialect in ["text_format", "response_format"] {
                settings.connection.api = "responses"; settings.connection.responsesSchemaFormat = dialect
                try await model.saveClassifierSettings(target, update: settings, credential: .keep)
                let saved = try await model.readClassifierSettings(target)
                try check(saved.requestProfile.isEmpty && saved.connection.responsesSchemaFormat == dialect, "Native protocol switch retained Chat-only profile")
            }
            settings.connection.api = "chat_completions"
            try await model.saveClassifierSettings(target, update: settings, credential: .keep)
        }
        var override = try await model.readClassifierSettings(a)
        override.connection.enabled = false
        try await model.saveClassifierSettings(a, update: override, credential: .keep)
        let grouped = await model.createGroup("Joining the conversation", members: [a, b])
        try check(grouped, "Could not create classifier group")
        try await wait(model, until: { !model.loading })
        let room = model.selectedID!
        model.sendMessage("hold listening: a quiet unrelated remark")
        try await wait(model, until: { model.participation.count == 2 && model.participation.allSatisfy { $0.status == "pending" } })
        try check(model.bubbles.count == 1 && model.pendingCount(room) == 2, "Deciding created a blank reply or lost observer")
        try await checkPendingBoundaries(model, persona: a, room: room)
        try await phase("participation-deciding")
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.count == 1 && model.participation.allSatisfy { $0.status == "listening" }, "False decision became a reply")
        try await phase("participation-listening")
        model.mentionedIDs = [a]
        model.sendMessage("volunteer: Mira is requested; Kai may join too")
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.count == 4 && model.bubbles.filter { !$0.isUser }.allSatisfy { $0.status == .complete }, "Dynamic speaker observer ended before generation completed")
        try check(Set(model.participation.map(\.status)) == Set(["mentioned", "selected"]), "Mention bypass/volunteer was not presented")
        try await phase("participation-volunteer")
        model.sendMessage("malformed synthetic result")
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.count == 5 && model.participation.allSatisfy { $0.status == "failed" && !$0.error.isEmpty }, "Classifier failure was hidden or created a reply")
        try await phase("participation-error")
        model.mentionedIDs = [a, b]; model.sendMessage("Manual recovery after a failed check")
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.count == 8 && model.bubbles.last?.status == .complete, "Explicit mentions did not recover independently")
        model.sendMessage("hold stop this decision")
        try await wait(model, until: { model.participation.count == 2 && model.participation.allSatisfy { $0.status == "pending" } })
        model.cancelParticipation(model.participation[0]); model.stopReply()
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.count == 9 && model.participation.allSatisfy { $0.status == "cancelled" }, "Stop checking lost observation or admitted late reply")
        try await phase("participation-stopped")
        try await phase("participation-settings")
        model.closeChat(); await model.load(); model.select(room)
        try await wait(model, until: { !model.loading })
        try check(model.bubbles.count == 9 && model.participation.allSatisfy { $0.status == "cancelled" }, "Restart lost durable decisions")
        model.closeChat()
        print("Native classifier checks passed: separate settings/protocol round trips, Deciding/Listening, @ bypass plus dynamic volunteer, visible errors, manual recovery, cancellation, durable reopening.")
    }
}
