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

    func create(name: String) throws -> PersonaRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.createPersona(name: name)
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
        case .queued: return "Queued"
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
    var showCharacter = false
    var showNewCharacter = false
    var replyRevision = 0
    private var rooms: [String: RoomState] = [:]
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
    private var selectedRoom: RoomState? { selectedID.map { room($0) } }
    var draft: String { get { selectedRoom?.draft ?? "" } set { selectedRoom?.draft = newValue } }
    var bubbles: [ChatBubble] { selectedRoom?.bubbles ?? [] }
    var error: String? { get { if let selectedRoom { return selectedRoom.error }; return openingError } set { if let selectedRoom { selectedRoom.error = newValue } else { openingError = newValue } } }
    private var openingError: String?
    var isReplying: Bool { !(selectedRoom?.runs.isEmpty ?? true) }
    var hasActiveReplies: Bool { rooms.values.contains { !$0.runs.isEmpty } }
    func pendingCount(_ personaID: String) -> Int { rooms[personaID]?.runs.count ?? 0 }
    private func room(_ id: String) -> RoomState {
        if let room = rooms[id] { return room }
        let state = RoomState(); rooms[id] = state; return state
    }

    func load() async {
        let revision = UUID(); selectionRevision = revision
        loading = true; openingError = nil
        do {
            let loaded = try await client.load()
            guard selectionRevision == revision else { return }
            workspace = loaded
            if !loaded.personas.contains(where: { $0.id == selectedID }) { selectedID = loaded.personas.first?.id }
            if let id = selectedID {
                let state = room(id)
                if state.runs.isEmpty {
                    let messages = try await client.history(persona: id)
                    guard selectionRevision == revision else { return }
                    if state.runs.isEmpty { state.bubbles = messages.map(ChatBubble.init) }
                }
                state.loaded = true
            }
        } catch { if selectionRevision == revision { self.error = describe(error) } }
        if selectionRevision == revision { loading = false }
    }

    func select(_ id: String) {
        guard selectedID != id else { return }
        selectionRevision = UUID(); selectedID = id
        loading = !room(id).loaded
        if loading { Task { await load() } }
    }

    func createCharacter(_ name: String) async -> Bool {
        do {
            let persona = try await client.create(name: name)
            workspace = try await client.load()
            select(persona.id)
            return true
        } catch { self.error = describe(error); return false }
    }

    func sendMessage(_ suggested: String? = nil) {
        guard let persona = selected, !loading else { return }
        let state = room(persona.id)
        let prompt = (suggested ?? state.draft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty else { return }
        state.error = nil
        let run = RoomRun()
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        run.sink = sink; state.runs.append(run)
        let observer = StreamObserver(sink)
        run.task = Task { [weak self, client] in
            do {
                let handle = try await client.send(persona: persona.id, prompt: prompt, observer: observer)
                guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { handle.cancel(); sink.finish(); return }
                run.handle = handle
                // A room switch must not clear another room's draft.
                if state.draft.trimmingCharacters(in: .whitespacesAndNewlines) == prompt { state.draft = "" }
                state.apply(handle.messages(), conversation: persona.conversationId)
                if run.cancelRequested { handle.cancel() }
                for await update in stream {
                    guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { break }
                    guard update.runId == handle.id(), update.conversationId == persona.conversationId,
                          update.personaId == persona.id else { continue }
                    state.consume(update, conversation: persona.conversationId)
                    if self?.selectedID == persona.id { self?.replyRevision += 1 }
                }
                if !Task.isCancelled, state.runs.count == 1, state.runs.first?.id == run.id {
                    let messages = try await client.history(persona: persona.id)
                    if state.runs.count == 1, state.runs.first?.id == run.id { state.bubbles = messages.map(ChatBubble.init) }
                }
            } catch {
                sink.finish()
                if !Task.isCancelled { state.error = self?.describe(error) }
            }
            state.runs.removeAll { $0.id == run.id }
            run.handle = nil; run.sink = nil; run.task = nil
        }
    }

    func stopReply() { selectedRoom?.runs.first?.cancel() }
    func cancelReply(_ messageID: String) {
        selectedRoom?.runs.first(where: { $0.handle?.messages().last?.id == messageID })?.cancel()
    }
    func closeChat() {
        selectionRevision = UUID()
        for state in rooms.values {
            for run in state.runs { run.cancel(); run.task?.cancel(); run.sink?.finish() }
            state.runs.removeAll()
        }
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
