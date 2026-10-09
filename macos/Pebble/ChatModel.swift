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

    func history(persona: String) throws -> [MessageRecord] {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble's character files first.") }
        return try backend.messages(personaId: persona)
    }

    func send(persona: String, prompt: String, observer: ChatObserver) throws -> ChatHandle {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble's character files first.") }
        return try backend.sendMessage(personaId: persona, prompt: prompt, observer: observer)
    }
}

final class StreamObserver: ChatObserver {
    let continuation: AsyncStream<ChatUpdate>.Continuation

    init(_ continuation: AsyncStream<ChatUpdate>.Continuation) { self.continuation = continuation }

    func onUpdate(update: ChatUpdate) throws {
        switch continuation.yield(update) {
        case .terminated: throw ListenerError.Closed(detail: "The view closed.")
        case .dropped: throw ListenerError.Closed(detail: "The chat consumer fell behind. Reopen the chat to load saved messages.")
        case .enqueued: break
        @unknown default: break
        }
        if update.kind == .completed || update.kind == .cancelled || update.kind == .failed { continuation.finish() }
    }
}

struct ChatBubble: Identifiable {
    let id: String
    let sequence: Int64
    let isUser: Bool
    var text: String
    var status: MessageStatus

    init(_ message: MessageRecord) {
        id = message.id
        sequence = message.sequence
        isUser = message.senderKind == "user"
        text = message.content
        status = message.status
    }

    var statusLabel: String? {
        switch status {
        case .cancelled: return "Stopped"
        case .interrupted: return "Interrupted"
        case .failed: return "Failed"
        case .complete, .streaming: return nil
        }
    }
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
    var bubbles: [ChatBubble] = []
    var isReplying = false
    var showCharacter = false
    var replyRevision = 0
    private var chatHandle: ChatHandle?
    private var chatTask: Task<Void, Never>?
    private var continuation: AsyncStream<ChatUpdate>.Continuation?
    private var cancellationRequested = false
    private var operation = UUID()
    private var selectionRevision = UUID()

    init(dataRoot: URL? = nil) {
        if let dataRoot { self.dataRoot = dataRoot }
        else if let override = ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"] {
            self.dataRoot = URL(fileURLWithPath: override, isDirectory: true)
        } else {
            self.dataRoot = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Pebble", isDirectory: true)
        }
        client = CoreClient(root: self.dataRoot.path)
    }

    var selected: PersonaRecord? { workspace?.personas.first { $0.id == selectedID } }

    func load() async {
        guard !isReplying else { return }
        let revision = UUID()
        selectionRevision = revision
        loading = true
        error = nil
        do {
            let loaded = try await client.load()
            guard selectionRevision == revision else { return }
            workspace = loaded
            if !loaded.personas.contains(where: { $0.id == selectedID }) {
                selectedID = loaded.personas.first?.id
                bubbles = []
            }
            if let id = selectedID {
                let messages = try await client.history(persona: id)
                guard selectionRevision == revision else { return }
                bubbles = messages.map(ChatBubble.init)
            }
        } catch {
            guard selectionRevision == revision else { return }
            self.error = describe(error)
        }
        loading = false
    }

    func select(_ id: String) {
        guard selectedID != id, !isReplying else { return }
        selectedID = id
        bubbles = []
        draft = ""
        loading = true
        Task { await load() }
    }

    func sendMessage(_ suggested: String? = nil) {
        guard let persona = selected, !isReplying, !loading else { return }
        let prompt = (suggested ?? draft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }
        error = nil
        let token = UUID()
        operation = token
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        continuation = sink
        let observer = StreamObserver(sink)
        cancellationRequested = false
        isReplying = true
        chatTask = Task { [weak self, client] in
            do {
                let handle = try await client.send(persona: persona.id, prompt: prompt, observer: observer)
                guard !Task.isCancelled, self?.operation == token else { handle.cancel(); sink.finish(); return }
                self?.chatHandle = handle
                self?.draft = ""
                self?.apply(handle.messages())
                if self?.cancellationRequested == true { handle.cancel() }
                for await update in stream {
                    guard !Task.isCancelled, self?.operation == token else { break }
                    guard update.runId == handle.id(), update.conversationId == persona.conversationId,
                          update.personaId == persona.id else { continue }
                    self?.consume(update)
                }
                if !Task.isCancelled, self?.operation == token {
                    // The final snapshot also reconciles a disconnected callback with durable state.
                    let messages = try await client.history(persona: persona.id)
                    guard !Task.isCancelled, self?.operation == token else { return }
                    self?.bubbles = messages.map(ChatBubble.init)
                    self?.finishReply()
                }
            } catch {
                sink.finish()
                guard !Task.isCancelled, self?.operation == token else { return }
                self?.error = self?.describe(error)
                if let messages = try? await client.history(persona: persona.id), self?.operation == token {
                    self?.bubbles = messages.map(ChatBubble.init)
                }
                if self?.operation == token { self?.finishReply() }
            }
        }
    }

    private func apply(_ messages: [MessageRecord]) {
        guard let conversation = selected?.conversationId else { return }
        for message in messages where message.conversationId == conversation {
            let bubble = ChatBubble(message)
            if let index = bubbles.firstIndex(where: { $0.id == message.id }) { bubbles[index] = bubble }
            else { bubbles.append(bubble) }
        }
        bubbles.sort { $0.sequence < $1.sequence }
    }

    private func consume(_ update: ChatUpdate) {
        apply(update.messages)
        if update.kind == .textDelta, let index = bubbles.firstIndex(where: { $0.id == update.messageId }) {
            bubbles[index].text += update.text
        }
        if update.kind == .failed {
            error = update.text
            if let index = bubbles.firstIndex(where: { $0.id == update.messageId }) { bubbles[index].status = .failed }
        }
        replyRevision += 1
    }

    func stopReply() {
        cancellationRequested = true
        chatHandle?.cancel()
    }

    private func finishReply() {
        isReplying = false
        chatHandle = nil
        continuation = nil
        chatTask = nil
    }

    func closeChat() {
        operation = UUID()
        selectionRevision = UUID()
        chatHandle?.cancel()
        chatTask?.cancel()
        continuation?.finish()
        finishReply()
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
        case .Busy: return "Finish or stop the current reply first."
        case .UnknownPersona: return "This character is no longer available. Reload the character files."
        case .EmptyMessage: return "Write something before sending."
        case let .Generation(detail): return "Couldn't start the reply. \(detail)"
        }
    }
}
