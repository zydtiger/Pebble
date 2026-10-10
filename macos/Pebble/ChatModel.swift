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

    func settings(persona: String?) throws -> SettingsRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.settings(personaId: persona)
    }
    func saveSettings(persona: String?, update: SettingsUpdateRecord) throws -> WorkspaceRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.saveSettings(personaId: persona, update: update)
    }
    func responseSettings(persona: String) throws -> ResponseSettingsRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.responseSettings(personaId: persona)
    }
    func saveResponseSettings(persona: String, update: ResponseSettingsUpdateRecord) throws -> WorkspaceRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.saveResponseSettings(personaId: persona, update: update)
    }
    func retry(persona: String, reply: String, observer: ChatObserver) throws -> ChatHandle {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.retryMessage(personaId: persona, replyId: reply, observer: observer)
    }
    func freshContext(persona: String) throws -> PersonaRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.freshContext(personaId: persona)
    }

    func history(persona: String) throws -> [MessageRecord] {
        guard let backend else { throw BackendError.UnknownPersona }
        return try backend.messages(personaId: persona)
    }
    func send(persona: String, prompt: String, observer: ChatObserver) throws -> ChatHandle {
        guard let backend else { throw BackendError.UnknownPersona }
        return try backend.sendMessage(personaId: persona, prompt: prompt, observer: observer)
    }
    func history(room: String) throws -> [MessageRecord] {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.roomMessages(roomId: room)
    }
    func createGroup(title: String, members: [String]) throws -> RoomRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.createGroup(title: title, members: members)
    }
    func renameGroup(room: String, title: String) throws -> RoomRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.renameGroup(roomId: room, title: title)
    }
    func send(room: String, prompt: String, mentions: [String], observer: ChatObserver) throws -> ChatSubmissionRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.sendRoomMessage(roomId: room, prompt: prompt, mentions: mentions, observer: observer)
    }

}

final class StreamObserver: ChatObserver {
    let continuation: AsyncStream<ChatUpdate>.Continuation

    let finishOnTerminal: Bool
    init(_ continuation: AsyncStream<ChatUpdate>.Continuation, finishOnTerminal: Bool = true) { self.continuation = continuation; self.finishOnTerminal = finishOnTerminal }

    func onUpdate(update: ChatUpdate) throws {
        switch continuation.yield(update) {
        case .terminated: throw ListenerError.Closed(detail: "The view closed.")
        case .dropped: throw ListenerError.Closed(detail: "The chat consumer fell behind. Reopen the chat to load saved messages.")
        case .enqueued: break
        @unknown default: break
        }
        if finishOnTerminal && (update.kind == .completed || update.kind == .cancelled || update.kind == .failed) { continuation.finish() }
    }
}

struct ChatBubble: Identifiable {
    let id: String
    let sequence: Int64
    let isUser: Bool
    let senderID: String
    var text: String
    var status: MessageStatus
    var failureDetail: String
    var canRetry: Bool

    init(_ message: MessageRecord) {
        id = message.id
        sequence = message.sequence
        isUser = message.senderKind == "user"
        senderID = message.senderId
        text = message.content
        status = message.status
        failureDetail = message.failureDetail
        canRetry = message.canRetry
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
    var showNewGroup = false
    var showRenameGroup = false
    var showFreshContext = false
    var settingsTarget: SettingsTarget?
    var stateTarget: SettingsTarget?
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

    var selectedConversation: RoomRecord? { workspace?.rooms.first { $0.id == selectedID } }
    var selected: PersonaRecord? { workspace?.personas.first { $0.id == selectedConversation?.personaId } }
    var mentionedIDs: [String] { get { selectedRoom?.mentions ?? [] } set { selectedRoom?.mentions = newValue } }
    func speakerName(_ id: String) -> String { workspace?.personas.first { $0.id == id }?.displayName ?? "Character" }
    func memberLabel(_ id: String) -> String {
        let name = speakerName(id)
        return (selectedConversation?.members.filter { $0.displayName == name }.count ?? 0) > 1 ? "\(name) · \(id.prefix(6))" : name
    }
    func toggleMention(_ id: String) {
        if mentionedIDs.contains(id) { mentionedIDs.removeAll { $0 == id } } else { mentionedIDs.append(id) }
    }
    private func conversationID(_ id: String) -> String { workspace?.personas.first { $0.id == id }?.conversationId ?? id }
    private var selectedRoom: RoomState? { selectedID.map { room($0) } }
    var draft: String { get { selectedRoom?.draft ?? "" } set { selectedRoom?.draft = newValue } }
    var bubbles: [ChatBubble] { selectedRoom?.bubbles ?? [] }
    var error: String? { get { if let selectedRoom { return selectedRoom.error }; return openingError } set { if let selectedRoom { selectedRoom.error = newValue } else { openingError = newValue } } }
    private var openingError: String?
    var isReplying: Bool { !(selectedRoom?.runs.isEmpty ?? true) }
    var hasActiveReplies: Bool { rooms.values.contains { !$0.runs.isEmpty } }
    func canRetry(_ bubble: ChatBubble) -> Bool {
        guard bubble.canRetry, !loading else { return false }
        return !rooms.values.contains { state in
            state.runs.contains { run in
                if run.accepting { return run.acceptingPersonaIDs.contains(bubble.senderID) }
                return run.handles.contains { handle in
                    run.pendingIDs.contains(handle.id()) && handle.messages().last?.senderId == bubble.senderID
                }
            }
        }
    }
    func pendingCount(_ id: String) -> Int { rooms[conversationID(id)]?.runs.reduce(0) { $0 + ($1.accepting ? 1 : $1.pendingIDs.count) } ?? 0 }
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
            if !loaded.rooms.contains(where: { $0.id == selectedID }) { selectedID = loaded.personas.first?.conversationId ?? loaded.rooms.first?.id }
            if let id = selectedID {
                let state = room(id)
                if state.runs.isEmpty {
                    let messages = try await client.history(room: id)
                    guard selectionRevision == revision else { return }
                    if state.runs.isEmpty { state.bubbles = messages.map(ChatBubble.init) }
                }
                state.loaded = true
            }
        } catch { if selectionRevision == revision { self.error = describe(error) } }
        if selectionRevision == revision { loading = false }
    }

    func select(_ target: String) {
        let id = conversationID(target)
        guard selectedID != id else { return }
        let revision = UUID(); selectionRevision = revision; selectedID = id
        let state = room(id)
        loading = !state.loaded
        if loading { Task { await load() } }
        else {
            Task {
                do {
                    let messages = try await client.history(room: id)
                    guard selectionRevision == revision, state.runs.isEmpty else { return }
                    state.bubbles = messages.map(ChatBubble.init)
                } catch { if selectionRevision == revision { state.error = describe(error) } }
            }
        }
    }
    func createGroup(_ title: String, members: [String]) async -> Bool {
        do {
            let group = try await client.createGroup(title: title, members: members)
            workspace = try await client.load(); select(group.id); return true
        } catch { self.error = describe(error); return false }
    }
    func renameGroup(_ title: String) async -> Bool {
        guard let id = selectedConversation?.id else { return false }
        do { _ = try await client.renameGroup(room: id, title: title); workspace = try await client.load(); return true }
        catch { self.error = describe(error); return false }
    }

    func createCharacter(_ name: String) async -> Bool {
        do {
            let persona = try await client.create(name: name)
            workspace = try await client.load()
            select(persona.id)
            return true
        } catch { self.error = describe(error); return false }
    }

    func startFreshContext() async -> Bool {
        guard let persona = selected, let id = selectedID else { return false }
        let state = room(id)
        do {
            _ = try await client.freshContext(persona: persona.id)
            workspace = try await client.load()
            state.error = nil
            return true
        } catch { state.error = describe(error); return false }
    }

    func readSettings(_ persona: String?) async throws -> SettingsRecord { try await client.settings(persona: persona) }
    func readResponseSettings(_ persona: String) async throws -> ResponseSettingsRecord { try await client.responseSettings(persona: persona) }
    func saveResponseSettings(_ persona: String, update: ResponseSettingsUpdateRecord) async throws {
        workspace = try await client.saveResponseSettings(persona: persona, update: update)
    }
    func saveSettings(_ persona: String?, update: SettingsUpdateRecord) async throws {
        workspace = try await client.saveSettings(persona: persona, update: update)
    }
    func retryReply(_ reply: String) {
        guard let conversation = selectedConversation, !loading,
              let bubble = bubbles.first(where: { $0.id == reply }), canRetry(bubble) else { return }
        beginReply(conversation, prompt: "", mentions: [], retry: (bubble.senderID, reply))
    }
    func sendMessage(_ suggested: String? = nil) {
        guard let conversation = selectedConversation, !loading else { return }
        let state = room(conversation.id)
        let prompt = (suggested ?? state.draft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !prompt.isEmpty || !state.mentions.isEmpty else { return }
        beginReply(conversation, prompt: prompt, mentions: state.mentions, retry: nil)
    }
    private func beginReply(_ conversation: RoomRecord, prompt: String, mentions: [String], retry: (String, String)?) {
        let state = room(conversation.id)
        state.error = nil
        let run = RoomRun()
        // Before Rust resolves typed mentions, conservatively cover possible speakers.
        // Once accepted, only the returned pending handles determine persona busy state.
        if let retry { run.acceptingPersonaIDs = [retry.0] }
        else if let persona = conversation.personaId { run.acceptingPersonaIDs = [persona] }
        else { run.acceptingPersonaIDs = Set(conversation.members.map(\.personaId)) }
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        run.sink = sink; state.runs.append(run)
        let observer = StreamObserver(sink, finishOnTerminal: false)
        run.task = Task { [weak self, client] in
            do {
                let handles: [ChatHandle]
                if let retry { handles = [try await client.retry(persona: retry.0, reply: retry.1, observer: observer)] }
                else {
                    let submission = try await client.send(room: conversation.id, prompt: prompt, mentions: mentions, observer: observer)
                    state.apply([submission.message], conversation: conversation.id)
                    handles = submission.handles
                }
                guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { handles.forEach { $0.cancel() }; sink.finish(); return }
                run.handles = handles; run.accepting = false; run.pendingIDs = Set(handles.map { $0.id() })
                if retry == nil {
                    if state.draft.trimmingCharacters(in: .whitespacesAndNewlines) == prompt { state.draft = "" }
                    if state.mentions == mentions { state.mentions = [] }
                }
                for handle in handles { state.apply(handle.messages(), conversation: conversation.id) }
                if run.cancelRequested { handles.forEach { $0.cancel() } }
                if handles.isEmpty { sink.finish() }
                for await update in stream {
                    guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { break }
                    guard handles.contains(where: { $0.id() == update.runId }), update.conversationId == conversation.id,
                          handles.contains(where: { $0.id() == update.runId && $0.messages().last?.senderId == update.personaId }) else { continue }
                    state.consume(update, conversation: conversation.id)
                    if self?.selectedID == conversation.id { self?.replyRevision += 1 }
                    if update.kind == .completed || update.kind == .failed || update.kind == .cancelled {
                        run.pendingIDs.remove(update.runId)
                        if run.pendingIDs.isEmpty { sink.finish() }
                    }
                }
                if !Task.isCancelled, state.runs.count == 1, state.runs.first?.id == run.id {
                    let messages = try await client.history(room: conversation.id)
                    if state.runs.count == 1, state.runs.first?.id == run.id { state.bubbles = messages.map(ChatBubble.init) }
                }
            } catch {
                sink.finish()
                if !Task.isCancelled { state.error = self?.describe(error) }
            }
            state.runs.removeAll { $0.id == run.id }
            run.handles = []; run.sink = nil; run.task = nil
            // Retry eligibility is persona-wide, including rooms with other active speakers.
            if !Task.isCancelled { await self?.refreshRoomsAfterReply() }
        }
    }
    private func refreshRoomsAfterReply() async {
        for (id, state) in rooms where state.loaded {
            do {
                let messages = try await client.history(room: id)
                if state.runs.isEmpty { state.bubbles = messages.map(ChatBubble.init) }
                else {
                    // Keep active stream content intact; refresh only Rust's derived eligibility.
                    let eligibility = Dictionary(uniqueKeysWithValues: messages.map { ($0.id, $0.canRetry) })
                    for index in state.bubbles.indices {
                        if let canRetry = eligibility[state.bubbles[index].id] { state.bubbles[index].canRetry = canRetry }
                    }
                }
            }
            catch { state.error = describe(error) }
        }
    }
    func stopReply() { selectedRoom?.runs.first?.cancel() }
    func cancelReply(_ messageID: String) {
        for run in selectedRoom?.runs ?? [] {
            if let handle = run.handles.first(where: { $0.messages().last?.id == messageID }) { handle.cancel(); return }
        }
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
    func describe(_ error: Error) -> String {
        guard let error = error as? BackendError else { return error.localizedDescription }
        switch error {
        case let .Configuration(path, detail): return "Check \(URL(fileURLWithPath: path).lastPathComponent)\n\(detail)\n\n\(path)"
        case let .Storage(detail): return detail
        case let .ContextLimit(detail): return detail
        case let .Settings(detail): return detail
        case let .RetryUnavailable(detail): return detail
        case .Busy: return "Finish or stop the current reply first."
        case .UnknownPersona: return "This character is no longer available. Reload the character files."
        case .EmptyMessage: return "Write something before sending."
        case let .Generation(detail): return "Couldn't start the reply. \(detail)"
        }
    }
}
