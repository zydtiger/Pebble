import Foundation

struct CheckFailure: Error, CustomStringConvertible {
    let description: String
}

func require(_ condition: @autoclosure () -> Bool, _ message: String) throws {
    if !condition() { throw CheckFailure(description: message) }
}

@main
struct BridgeChecks {
    @MainActor
    static func main() async throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("pebble-bridge-\(UUID())")
        defer { try? FileManager.default.removeItem(at: root) }
        let client = CoreClient(root: root.path)
        let first = try await client.load()
        let reopened = try PebbleBackend(dataRoot: root.path).workspace()
        try require(first.personas.count == 1, "Expected the starter character")
        let persona = first.personas[0]
        try require(persona.id == reopened.personas[0].id, "Persona identity changed on restart")
        try require(persona.conversationId == reopened.personas[0].conversationId, "Private room changed on restart")

        let (stream, sink) = AsyncStream<PreviewUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        var observer: StreamObserver? = StreamObserver(sink)
        weak let weakObserver = observer
        var handle: PreviewHandle? = try await client.preview(persona: persona.id, prompt: "你好", observer: observer!)
        let runID = handle!.id()
        observer = nil
        var sequence: UInt64 = 0
        var text = ""
        var terminal = false
        for await update in stream {
            try require(update.sequence == sequence, "Event order changed at the Swift boundary")
            try require(update.runId == runID, "Wrong preview ID")
            try require(update.conversationId == persona.conversationId, "Wrong room")
            sequence += 1
            text += update.text
            if update.kind == .completed { terminal = true }
        }
        try require(terminal && text.contains("你好，我是 Mira。"), "Unicode stream did not complete")
        handle = nil
        for _ in 0..<50 where weakObserver != nil { try await Task.sleep(for: .milliseconds(10)) }
        try require(weakObserver == nil, "Rust retained the Swift listener after completion")

        let (cancelStream, cancelSink) = AsyncStream<PreviewUpdate>.makeStream()
        let cancelled = try await client.preview(persona: persona.id, prompt: "hello", observer: StreamObserver(cancelSink))
        cancelled.cancel()
        var cancelledTerminal = false
        for await update in cancelStream {
            if update.kind == .cancelled { cancelledTerminal = true }
            try require(update.kind != .completed, "Cancellation became completion")
        }
        try require(cancelledTerminal, "Rust did not acknowledge cancellation")

        do {
            let (unused, unusedSink) = AsyncStream<PreviewUpdate>.makeStream()
            _ = unused
            _ = try await client.preview(persona: "not-a-persona", prompt: "hello", observer: StreamObserver(unusedSink))
            throw CheckFailure(description: "Missing typed error")
        } catch BackendError.UnknownPersona { }

        // Exercise the real UI adapter's cancellation before a handle returns.
        let model = ChatModel()
        await model.load()
        try require(model.selected != nil, "Presentation adapter did not load a character")
        model.startPreview("hello")
        model.stopPreview()
        for _ in 0..<100 where model.isPreviewing { try await Task.sleep(for: .milliseconds(20)) }
        try require(!model.isPreviewing, "Early UI cancellation was lost")
        try require(model.bubbles.last?.stopped == true, "UI did not show cancellation")
        model.startPreview("another hello")
        model.closePreview()
        try await Task.sleep(for: .milliseconds(100))
        try require(!model.isPreviewing, "Closing the subscription left a preview running")

        print("Swift 6 FFI checks passed: typed records/errors, restart IDs, Unicode events, cancellation, listener release, view lifecycle.")
    }
}
