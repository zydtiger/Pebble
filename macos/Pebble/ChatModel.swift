import AppKit
import Foundation
import Observation

actor CoreClient {
    private var backend: PebbleBackend?
    private let root: String

    init(root: String) { self.root = root }

    func load() throws -> WorkspaceRecord {
        if backend == nil { backend = try PebbleBackend(dataRoot: root) }
        return try backend!.workspace()
    }

    func preview(persona: String, prompt: String, observer: PreviewObserver) throws -> PreviewHandle {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble's character files first.") }
        return try backend.startPreview(personaId: persona, prompt: prompt, observer: observer)
    }
}

final class StreamObserver: PreviewObserver {
    let continuation: AsyncStream<PreviewUpdate>.Continuation

    init(_ continuation: AsyncStream<PreviewUpdate>.Continuation) {
        self.continuation = continuation
    }

    func onUpdate(update: PreviewUpdate) throws {
        switch continuation.yield(update) {
        case .terminated:
            throw ListenerError.Closed(detail: "The view closed.")
        case .dropped:
            throw ListenerError.Closed(detail: "The preview consumer fell behind. Please try again.")
        case .enqueued:
            break
        @unknown default:
            break
        }
        if update.kind == .completed || update.kind == .cancelled { continuation.finish() }
    }
}

struct PreviewBubble: Identifiable {
    let id = UUID()
    let isUser: Bool
    var text: String
    var stopped = false
}

@MainActor
@Observable
final class ChatModel {
    let dataRoot: URL
    private let client: CoreClient
    var workspace: WorkspaceRecord?
    var selectedID: String?
    var loading = true
    var error: String?
    var draft = ""
    var bubbles: [PreviewBubble] = []
    var isPreviewing = false
    var showCharacter = false
    var previewRevision = 0
    private var previewHandle: PreviewHandle?
    private var previewTask: Task<Void, Never>?
    private var continuation: AsyncStream<PreviewUpdate>.Continuation?
    private var cancellationRequested = false

    init() {
        if let override = ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"] {
            dataRoot = URL(fileURLWithPath: override, isDirectory: true)
        } else {
            dataRoot = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Pebble", isDirectory: true)
        }
        client = CoreClient(root: dataRoot.path)
    }

    var selected: PersonaRecord? {
        workspace?.personas.first { $0.id == selectedID }
    }

    func load() async {
        loading = true
        error = nil
        do {
            let loaded = try await client.load()
            workspace = loaded
            if !loaded.personas.contains(where: { $0.id == selectedID }) {
                closePreview()
                bubbles = []
                selectedID = loaded.personas.first?.id
            }
        } catch {
            self.error = describe(error)
        }
        loading = false
    }

    func select(_ id: String) {
        guard selectedID != id else { return }
        closePreview()
        bubbles = []
        selectedID = id
        draft = ""
    }

    func startPreview(_ suggested: String? = nil) {
        guard let persona = selected, !isPreviewing else { return }
        let prompt = (suggested ?? draft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }
        error = nil
        draft = ""
        bubbles.append(PreviewBubble(isUser: true, text: prompt))
        bubbles.append(PreviewBubble(isUser: false, text: ""))
        let replyID = bubbles.last!.id
        let (stream, sink) = AsyncStream<PreviewUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        continuation = sink
        let observer = StreamObserver(sink)
        cancellationRequested = false
        isPreviewing = true
        previewTask = Task { [weak self, client] in
            do {
                let handle = try await client.preview(persona: persona.id, prompt: prompt, observer: observer)
                guard !Task.isCancelled else { handle.cancel(); sink.finish(); return }
                self?.previewHandle = handle
                if self?.cancellationRequested == true { handle.cancel() }
                for await update in stream {
                    guard !Task.isCancelled else { break }
                    guard update.runId == handle.id(), update.conversationId == persona.conversationId else { continue }
                    self?.consume(update, replyID: replyID)
                }
                if !Task.isCancelled { self?.finishPreview() }
            } catch {
                sink.finish()
                if !Task.isCancelled {
                    self?.error = self?.describe(error)
                    self?.bubbles.removeAll { $0.id == replyID && $0.text.isEmpty }
                    self?.finishPreview()
                }
            }
        }
    }

    private func consume(_ update: PreviewUpdate, replyID: UUID) {
        guard let index = bubbles.firstIndex(where: { $0.id == replyID }) else { return }
        switch update.kind {
        case .textDelta:
            bubbles[index].text += update.text
        case .cancelled:
            bubbles[index].stopped = true
        case .started, .completed:
            break
        }
        previewRevision += 1
    }

    func stopPreview() {
        cancellationRequested = true
        previewHandle?.cancel()
    }

    private func finishPreview() {
        isPreviewing = false
        previewHandle = nil
        continuation = nil
        previewTask = nil
    }

    func closePreview() {
        previewHandle?.cancel()
        previewTask?.cancel()
        continuation?.finish()
        finishPreview()
    }

    func openFiles() { NSWorkspace.shared.open(dataRoot) }

    func openCharacterFiles() {
        guard let selected else { return }
        NSWorkspace.shared.open(URL(fileURLWithPath: selected.directory, isDirectory: true))
    }

    private func describe(_ error: Error) -> String {
        guard let error = error as? BackendError else { return error.localizedDescription }
        switch error {
        case let .Configuration(path, detail): return "Check \(URL(fileURLWithPath: path).lastPathComponent)\n\(detail)\n\n\(path)"
        case let .Storage(detail): return detail
        case .Busy: return "Finish or stop the current preview first."
        case .UnknownPersona: return "This character is no longer available. Reload the character files."
        case .EmptyMessage: return "Write something before starting a preview."
        case let .Preview(detail): return "Couldn't start the preview. \(detail)"
        }
    }
}
