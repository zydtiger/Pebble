import AppKit
import Foundation
import ImageIO
import Observation
import UniformTypeIdentifiers

actor CoreClient {
    private var backend: PebbleBackend?
    private let root: String

    init(root: String) { self.root = root }

    func requireBackend() throws -> PebbleBackend {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }; return backend
    }

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
    func classifierSettings(persona: String?) throws -> ClassifierSettingsRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.classifierSettings(personaId: persona)
    }
    func saveClassifierSettings(persona: String?, update: ClassifierSettingsRecord, credential: CredentialUpdate) throws -> WorkspaceRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.saveClassifierSettings(personaId: persona, update: update, credential: credential)
    }
    func participation(room: String) throws -> [ParticipationRecord] {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.participation(roomId: room)
    }
    func retry(persona: String, reply: String, observer: ChatObserver) throws -> ChatHandle {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.retryMessage(personaId: persona, replyId: reply, observer: observer)
    }
    func startFreshContext(persona: String) throws -> MaintenanceHandle {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.startFreshContext(personaId: persona)
    }
    func maintenanceStatus(persona: String) throws -> MaintenanceStatusRecord? {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.maintenanceStatus(personaId: persona)
    }
    func markers(room: String) throws -> [ContextMarkerRecord] {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.contextMarkers(roomId: room)
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
    func addGroupMember(room: String, persona: String) throws -> RoomRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.addGroupMember(roomId: room, personaId: persona)
    }
    func removeGroupMember(room: String, persona: String) throws -> RoomRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.removeGroupMember(roomId: room, personaId: persona)
    }
    func send(room: String, prompt: String, mentions: [String], images: [String] = [], separateTasks: Bool = false, observer: ChatObserver) throws -> ChatSubmissionRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        return try backend.sendRoomAssignedImages(roomId: room, prompt: prompt, mentions: mentions, images: images, separateTasks: separateTasks, observer: observer)
    }

    func importImage(_ source: ImageImportSource) throws -> ImageAssetRecord {
        guard let backend else { throw BackendError.Storage(detail: "Open Pebble first.") }
        switch source {
        case let .file(url): return try backend.importImage(path: url.path)
        case let .data(data): return try backend.importImageData(bytes: data)
        }
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
    var images: [ImageAssetRecord]

    init(_ message: MessageRecord) {
        id = message.id
        sequence = message.sequence
        isUser = message.senderKind == "user"
        senderID = message.senderId
        text = message.content
        status = message.status
        failureDetail = message.failureDetail
        canRetry = message.canRetry
        images = message.images
    }

    var statusLabel: String? {
        switch status {
        case .cancelled: return "Stopped"
        case .interrupted: return "Interrupted"
        case .failed: return "Failed"
        case .queued: return nil
        case .complete, .streaming: return nil
        }
    }
}

@MainActor
@Observable
final class ChatModel {
    let dataRoot: URL
    let client: CoreClient
    let replyNotifications: ReplyNotifications?
    private var pendingNotificationRoom: String?
    var workspace: WorkspaceRecord? { didSet { reconcileMentionChoices() } }
    var avatars: [String: NSImage] = [:]
    var selectedID: String?
    var loading = true
    var showCharacter = false
    var showNewCharacter = false
    var showNewGroup = false
    var showRenameGroup = false
    var showGroupMembers = false
    var showFreshContext = false
    var showImageGenerationSettings = false
    var imageGenerations: [ImageGenerationRecord] = []
    var imageGenerationStream: Task<Void, Never>?
    var imageGenerationSink: AsyncStream<[ImageGenerationRecord]>.Continuation?
    var showCodexSettings = false
    var delegatedTasks: [DelegatedTaskRecord] = []
    var personaActivities: [String: PersonaActivitySnapshotRecord] = [:]
    var activityObservation: Task<Void, Never>?
    var delegationStream: Task<Void, Never>?
    var delegationSink: AsyncStream<[DelegatedTaskRecord]>.Continuation?
    var settingsTarget: SettingsTarget?
    var stateTarget: SettingsTarget?
    var classifierTarget: SettingsTarget?
    var replyRevision = 0
    private var maintenanceHandles: [String: MaintenanceHandle] = [:]
    private var maintenanceObservation: Task<Void, Never>?
    private var observedMaintenance: [String: String] = [:]
    var contextFeedback: String?
    var maintenanceDetails: [String: String] = [:]
    var isMaintaining: Bool { selected.map { maintenanceHandles[$0.id] != nil } ?? false }
    var maintenanceDetail: String? { selected.flatMap { maintenanceDetails[$0.id] } }
    var markers: [ContextMarkerRecord] { selectedRoom?.markers ?? [] }
    func cancelMaintenance() { if let id = selected?.id { maintenanceHandles[id]?.cancel() } }
    private var rooms: [String: RoomState] = [:]
    private var selectionRevision = UUID()

    init(dataRoot: URL? = nil, notifications: ReplyNotifications? = nil) {
        replyNotifications = notifications
        if let dataRoot { self.dataRoot = dataRoot }
        else if let override = ProcessInfo.processInfo.environment["PEBBLE_DATA_DIR"] {
            self.dataRoot = URL(fileURLWithPath: override, isDirectory: true)
        } else {
            self.dataRoot = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
                .appendingPathComponent("Pebble", isDirectory: true)
        }
        client = CoreClient(root: self.dataRoot.path)
    }

    private var latestMessageTimes: [String: String] = [:]

    var sidebarRooms: [RoomRecord] {
        (workspace?.rooms ?? []).enumerated().sorted { lhs, rhs in
            let left = latestMessageTimes[lhs.element.id] ?? ""
            let right = latestMessageTimes[rhs.element.id] ?? ""
            return left == right ? lhs.offset < rhs.offset : left > right
        }.map(\.element)
    }

    private func observeMessageTimes(_ messages: [MessageRecord]) {
        for message in messages {
            latestMessageTimes[message.conversationId] = max(
                latestMessageTimes[message.conversationId] ?? "", message.createdAt)
        }
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
    var selectedRoom: RoomState? { selectedID.map { room($0) } }
    var separateTasks: Bool {
        get { selectedRoom?.separateTasks ?? false }
        set { selectedRoom?.separateTasks = newValue }
    }
    var draft: String { get { selectedRoom?.draft ?? "" } set { selectedRoom?.draft = newValue } }
    var draftImages: [ImageAssetRecord] { selectedRoom?.images ?? [] }
    var importingImages: Bool { (selectedRoom?.importingImages ?? 0) > 0 }
    func removeImage(_ id: String) { selectedRoom?.images.removeAll { $0.id == id } }
    func pickImages(panel: NSOpenPanel = NSOpenPanel()) {
        panel.allowedContentTypes = [.jpeg, .png]
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        guard panel.runModal() == .OK else { return }
        importImages(panel.urls.map(ImageImportSource.file))
    }
    func importImages(_ sources: [ImageImportSource]) {
        guard let id = selectedID, !sources.isEmpty else { return }
        let state = room(id)
        guard state.images.count + state.importingImages + sources.count <= 4 else {
            state.error = "Send at most four images in one message."; return
        }
        state.importingImages += sources.count
        Task { [client] in
            defer { state.importingImages -= sources.count }
            do {
                var imported: [ImageAssetRecord] = []
                for source in sources { imported.append(try await client.importImage(source)) }
                state.images.append(contentsOf: imported)
            } catch { state.error = describe(error) }
        }
    }
    var bubbles: [ChatBubble] {
        (selectedRoom?.bubbles ?? []).sorted {
            let lhsActive = !$0.isUser && ($0.status == .streaming || $0.status == .queued)
            let rhsActive = !$1.isUser && ($1.status == .streaming || $1.status == .queued)
            if lhsActive != rhsActive { return !lhsActive }
            return $0.sequence < $1.sequence
        }
    }
    var participation: [ParticipationRecord] {
        guard let latest = bubbles.last(where: { $0.isUser }) else { return [] }
        let records = selectedRoom?.participation ?? []
        // Keep protected older requests visible while a newer user round is active.
        let active = records.filter { $0.messageId != latest.id && ["pending", "admitted"].contains($0.slotStatus) }
        var latestByPersona: [String: ParticipationRecord] = [:]
        for record in records where record.messageId == latest.id { latestByPersona[record.personaId] = record }
        return active + latestByPersona.values.sorted { $0.personaId < $1.personaId }
    }
    func cancelParticipation(_ record: ParticipationRecord) {
        selectedRoom?.runs.first { $0.group?.id() == record.messageId }?.group?.cancelDecision(id: record.id)
    }

    var error: String? { get { if let selectedRoom { return selectedRoom.error }; return openingError } set { if let selectedRoom { selectedRoom.error = newValue } else { openingError = newValue } } }
    private var openingError: String?
    var isReplying: Bool { !(selectedRoom?.runs.isEmpty ?? true) }
    var hasActiveReplies: Bool { rooms.values.contains { !$0.runs.isEmpty } }
    func canRetry(_ bubble: ChatBubble) -> Bool {
        guard bubble.canRetry, !loading, maintenanceHandles[bubble.senderID] == nil else { return false }
        return !rooms.values.contains { state in
            if state.participation.contains(where: { $0.personaId == bubble.senderID && ["pending", "admitted"].contains($0.slotStatus) }) { return true }
            return state.runs.contains { run in
                if run.accepting { return run.acceptingPersonaIDs.contains(bubble.senderID) }
                return run.handles.contains { handle in
                    run.pendingIDs.contains(handle.id()) && handle.messages().last?.senderId == bubble.senderID
                }
            }
        }
    }
    func pendingCount(_ id: String) -> Int { rooms[conversationID(id)]?.runs.reduce(0) { $0 + ($1.accepting ? 1 : $1.pendingIDs.count + $1.decisionIDs.count) } ?? 0 }
    private func room(_ id: String) -> RoomState {
        if let room = rooms[id] { return room }
        let state = RoomState(); rooms[id] = state; return state
    }
    private func reconcileMentionChoices() {
        for conversation in workspace?.rooms ?? [] {
            guard let state = rooms[conversation.id] else { continue }
            let current = Set(conversation.members.map(\.personaId))
            let retained = state.mentions.filter { current.contains($0) }
            if retained != state.mentions { state.mentions = retained }
        }
    }

    func load() async {
        let revision = UUID(); selectionRevision = revision
        loading = true; openingError = nil
        do {
            let loaded = try await client.load()
            guard selectionRevision == revision else { return }
            let previews = await Task.detached(priority: .utility) {
                Self.loadAvatars(loaded.personas)
            }.value
            guard selectionRevision == revision else { return }
            avatars = previews.mapValues { NSImage(cgImage: $0, size: .zero) }
            workspace = loaded
            // Establish every room's history before subscribing, including rooms never opened.
            // The initial task snapshot then catches reports accepted in the subscription gap.
            for conversation in loaded.rooms {
                let needsHistory = latestMessageTimes[conversation.id] == nil
                    || replyNotifications?.needsBaseline(conversation.id) == true
                if needsHistory {
                    let messages = try await client.history(room: conversation.id)
                    observeMessageTimes(messages)
                    latestMessageTimes[conversation.id] = latestMessageTimes[conversation.id] ?? ""
                    replyNotifications?.baseline(messages, room: conversation.id)
                }
            }
            startMaintenanceObservation()
            await reloadActivities()
            startActivityObservation()
            try await startTaskObservation()
            try await startImageObservation()
            for persona in loaded.personas where maintenanceHandles[persona.id] == nil {
                if let status = try await client.maintenanceStatus(persona: persona.id) {
                    maintenanceDetails[persona.id] = status.detail
                    if ["failed", "cancelled", "interrupted"].contains(status.status) {
                        room(persona.conversationId).error = status.detail
                    }
                }
            }
            if !loaded.rooms.contains(where: { $0.id == selectedID }) { selectedID = loaded.personas.first?.conversationId ?? loaded.rooms.first?.id }
            if let id = selectedID {
                let state = room(id)
                if state.runs.isEmpty {
                    let messages = try await client.history(room: id)
                    observeMessageTimes(messages)
                    let decisions = try await client.participation(room: id)
                    guard selectionRevision == revision else { return }
                    if state.runs.isEmpty { state.bubbles = messages.map(ChatBubble.init); state.participation = decisions; state.markers = try await client.markers(room: id) }
                }
                state.loaded = true
            }
        } catch { if selectionRevision == revision { self.error = describe(error) } }
        if selectionRevision == revision {
            loading = false
            openPendingNotificationRoom()
        }
    }

    func openNotificationRoom(_ id: String) {
        pendingNotificationRoom = id
        openPendingNotificationRoom()
    }

    private func openPendingNotificationRoom() {
        guard !loading, let workspace, let id = pendingNotificationRoom else { return }
        pendingNotificationRoom = nil
        guard workspace.rooms.contains(where: { $0.id == id }) else { return }
        showCharacter = false; showNewCharacter = false; showNewGroup = false
        showRenameGroup = false; showGroupMembers = false; showFreshContext = false
        showCodexSettings = false; settingsTarget = nil; stateTarget = nil; classifierTarget = nil
        select(id)
    }

    func notifyCompletedReplies(_ messages: [MessageRecord], room id: String) {
        guard let conversation = workspace?.rooms.first(where: { $0.id == id }) else { return }
        replyNotifications?.completed(messages, room: conversation, speakerName: speakerName)
    }

    // Presentation-only previews, refreshed by the same explicit file reload as the persona.
    private nonisolated static func loadAvatars(_ personas: [PersonaRecord]) -> [String: CGImage] {
        var images: [String: CGImage] = [:]
        for persona in personas {
            let url = URL(fileURLWithPath: persona.directory, isDirectory: true)
                .appendingPathComponent("avatar.png")
            guard FileManager.default.fileExists(atPath: url.path),
                  let source = CGImageSourceCreateWithURL(url as CFURL,
                      [kCGImageSourceShouldCache: false] as CFDictionary),
                  let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [
                      kCGImageSourceCreateThumbnailFromImageAlways: true,
                      kCGImageSourceCreateThumbnailWithTransform: true,
                      kCGImageSourceThumbnailMaxPixelSize: 256,
                  ] as CFDictionary) else { continue }
            images[persona.id] = image
        }
        return images
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
                    observeMessageTimes(messages)
                    let decisions = try await client.participation(room: id)
                    guard selectionRevision == revision, state.runs.isEmpty else { return }
                    state.bubbles = messages.map(ChatBubble.init)
                    state.participation = decisions
                    state.markers = try await client.markers(room: id)
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
    func addGroupMember(_ persona: String, room: String) async throws {
        _ = try await client.addGroupMember(room: room, persona: persona)
        workspace = try await client.load()
        await refreshRoomsAfterReply()
    }
    func removeGroupMember(_ persona: String, room: String) async throws {
        _ = try await client.removeGroupMember(room: room, persona: persona)
        workspace = try await client.load()
        await refreshRoomsAfterReply()
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
        guard let persona = selected, let id = selectedID, maintenanceHandles[persona.id] == nil else { return false }
        let state = room(id)
        do {
            let handle = try await client.startFreshContext(persona: persona.id)
            maintenanceHandles[persona.id] = handle
            state.error = nil
            while true {
                let status = handle.status()
                maintenanceDetails[persona.id] = status.detail
                if status.status != "running" {
                    maintenanceHandles[persona.id] = nil
                    workspace = try await client.load()
                    await refreshRoomsAfterReply()
                    if status.status != "completed" { state.error = status.detail; return false }
                    state.error = nil
                    contextFeedback = status.memoryReviewed ? "Memory reviewed. Fresh context started." : "Fresh context started."
                    Task { @MainActor [weak self] in
                        try? await Task.sleep(for: .seconds(3))
                        self?.contextFeedback = nil
                    }
                    return true
                }
                try await Task.sleep(for: .milliseconds(100))
            }
        } catch {
            maintenanceHandles[persona.id]?.cancel()
            maintenanceHandles[persona.id] = nil
            if case let BackendError.Generation(detail) = error {
                state.error = "Couldn't start a fresh context. \(detail)"
            } else { state.error = describe(error) }
            return false
        }
    }

    func readSettings(_ persona: String?) async throws -> SettingsRecord { try await client.settings(persona: persona) }
    func readResponseSettings(_ persona: String) async throws -> ResponseSettingsRecord { try await client.responseSettings(persona: persona) }
    func saveResponseSettings(_ persona: String, update: ResponseSettingsUpdateRecord) async throws {
        workspace = try await client.saveResponseSettings(persona: persona, update: update)
    }
    func saveSettings(_ persona: String?, update: SettingsUpdateRecord) async throws {
        workspace = try await client.saveSettings(persona: persona, update: update)
    }
    func readClassifierSettings(_ persona: String?) async throws -> ClassifierSettingsRecord { try await client.classifierSettings(persona: persona) }
    func saveClassifierSettings(_ persona: String?, update: ClassifierSettingsRecord, credential: CredentialUpdate) async throws {
        workspace = try await client.saveClassifierSettings(persona: persona, update: update, credential: credential)
    }
    func retryReply(_ reply: String) {
        guard let conversation = selectedConversation, !loading,
              let bubble = bubbles.first(where: { $0.id == reply }), canRetry(bubble) else { return }
        beginReply(conversation, prompt: "", usesDraft: false, retry: (bubble.senderID, reply))
    }
    func sendMessage(_ suggested: String? = nil) {
        guard let conversation = selectedConversation, !loading else { return }
        let state = room(conversation.id)
        let prompt = (suggested ?? state.draft).trimmingCharacters(in: .whitespacesAndNewlines)
        let usesDraft = suggested == nil
        guard !importingImages, !prompt.isEmpty || (usesDraft && (!state.mentions.isEmpty || !state.images.isEmpty)) else { return }
        beginReply(conversation, prompt: prompt, usesDraft: usesDraft, retry: nil)
    }
    private func beginReply(_ conversation: RoomRecord, prompt: String, usesDraft: Bool, retry: (String, String)?) {
        replyNotifications?.requestAuthorization()
        let state = room(conversation.id)
        let acceptedDraft = state.draft
        let draftRevision = state.draftRevision
        let mentionRevision = state.mentionRevision
        let imageRevision = state.imageRevision
        let delegationRevision = state.delegationRevision
        let mentions = usesDraft ? state.mentions : []
        let separate = usesDraft && state.separateTasks
        let images = usesDraft ? state.images.map(\.id) : []
        state.error = nil
        let run = RoomRun()
        // Before Rust resolves typed mentions, conservatively cover possible speakers.
        // Once accepted, durable slots and returned pending handles determine persona busy state.
        if let retry { run.acceptingPersonaIDs = [retry.0] }
        else if let persona = conversation.personaId { run.acceptingPersonaIDs = [persona] }
        else { run.acceptingPersonaIDs = Set(conversation.members.map(\.personaId)) }
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream(bufferingPolicy: .bufferingOldest(256))
        run.sink = sink; state.runs.append(run)
        let observer = StreamObserver(sink, finishOnTerminal: false)
        run.task = Task { [weak self, client] in
            do {
                let handles: [ChatHandle]
                if let retry {
                    handles = [try await client.retry(persona: retry.0, reply: retry.1, observer: observer)]
                    let messages = try await client.history(room: conversation.id)
                    self?.observeMessageTimes(messages)
                    if !run.superseded { state.bubbles = messages.map(ChatBubble.init) }
                }
                else {
                    let submission = try await client.send(room: conversation.id, prompt: prompt, mentions: mentions, images: images, separateTasks: separate, observer: observer)
                    self?.observeMessageTimes([submission.message])
                    if submission.message.sequence >= state.latestReceiptSequence {
                        state.latestReceiptSequence = submission.message.sequence
                        for previous in state.runs where previous.id != run.id && !previous.accepting { previous.superseded = true }
                    } else { run.superseded = true }
                    handles = submission.handles
                    run.group = submission.group; run.classifying = submission.group != nil
                    run.decisionIDs = Set(submission.participation.filter { $0.slotStatus == "pending" }.map(\.id))
                    if !run.superseded { state.applyParticipation(submission.participation, conversation: conversation.id) }
                    let messages = try await client.history(room: conversation.id)
                    self?.observeMessageTimes(messages)
                    let decisions = try await client.participation(room: conversation.id)
                    if submission.message.sequence < state.latestReceiptSequence { run.superseded = true }
                    if !run.superseded {
                        state.bubbles = messages.map(ChatBubble.init)
                        state.participation = decisions
                    }
                }
                guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { handles.forEach { $0.cancel() }; sink.finish(); return }
                run.handles = handles; run.accepting = false; run.pendingIDs = Set(handles.map { $0.id() })
                if retry == nil && usesDraft {
                    if state.draftRevision == draftRevision && state.mentionRevision == mentionRevision && state.imageRevision == imageRevision && state.delegationRevision == delegationRevision && state.draft == acceptedDraft {
                        if state.draft.trimmingCharacters(in: .whitespacesAndNewlines) == prompt { state.draft = "" }
                        state.mentions = []; state.images = []; state.separateTasks = false
                    }
                }
                if !run.superseded { for handle in handles { state.apply(handle.messages(), conversation: conversation.id) } }
                if run.cancelRequested { run.cancel() }
                if handles.isEmpty && !run.classifying { sink.finish() }
                for await update in stream {
                    guard !Task.isCancelled, state.runs.contains(where: { $0.id == run.id }) else { break }
                    guard update.conversationId == conversation.id else { continue }
                    if !run.superseded { self?.observeMessageTimes(update.messages) }
                    if update.kind == .participation || update.kind == .admitted || update.kind == .groupSettled {
                        guard let group = run.group else { continue }
                        if update.kind == .groupSettled {
                            guard update.runId == group.id() else { continue }
                            run.classifying = false; run.decisionIDs = []
                        } else {
                            guard update.participation.allSatisfy({ $0.messageId == group.id() && $0.conversationId == conversation.id }) else { continue }
                            if !run.superseded { state.applyParticipation(update.participation, conversation: conversation.id) }
                            for record in update.participation {
                                if record.slotStatus == "pending" { run.decisionIDs.insert(record.id) }
                                else { run.decisionIDs.remove(record.id) }
                            }
                            if update.kind == .admitted {
                                for handle in group.handles() where !run.handles.contains(where: { $0.id() == handle.id() }) {
                                    run.handles.append(handle)
                                    if !run.completedIDs.contains(handle.id()) { run.pendingIDs.insert(handle.id()) }
                                }
                                if !run.superseded { state.apply(update.messages, conversation: conversation.id) }
                            }
                        }
                        if !run.classifying && run.pendingIDs.isEmpty { sink.finish() }
                        continue
                    }
                    guard run.handles.contains(where: { $0.id() == update.runId }), update.conversationId == conversation.id,
                          run.handles.contains(where: { $0.id() == update.runId && $0.messages().last?.senderId == update.personaId }) else { continue }
                    if !run.superseded { state.consume(update, conversation: conversation.id) }
                    if !run.superseded && update.kind == .completed {
                        self?.notifyCompletedReplies(update.messages.filter {
                            $0.id == update.messageId && $0.senderId == update.personaId
                        }, room: conversation.id)
                    }
                    if self?.selectedID == conversation.id { self?.replyRevision += 1 }
                    if update.kind == .completed || update.kind == .failed || update.kind == .cancelled {
                        run.completedIDs.insert(update.runId)
                        run.pendingIDs.remove(update.runId)
                        if !run.classifying && run.pendingIDs.isEmpty { sink.finish() }
                    }
                }
                if !Task.isCancelled, state.runs.count == 1, state.runs.first?.id == run.id {
                    let messages = try await client.history(room: conversation.id)
                    self?.observeMessageTimes(messages)
                    if state.runs.count == 1, state.runs.first?.id == run.id { state.bubbles = messages.map(ChatBubble.init) }
                }
            } catch {
                sink.finish()
                if !Task.isCancelled && !run.superseded { state.error = self?.describe(error) }
            }
            state.runs.removeAll { $0.id == run.id }
            run.handles = []; run.group = nil; run.sink = nil; run.task = nil
            // Retry eligibility is persona-wide, including rooms with other active speakers.
            if !Task.isCancelled { await self?.refreshRoomsAfterReply() }
        }
    }
    func refreshRoomsAfterReply(notificationRooms: Set<String> = []) async {
        let ids = Set(rooms.filter { $0.value.loaded }.map(\.key)).union(notificationRooms)
        for id in ids {
            do {
                let messages = try await client.history(room: id)
                guard !Task.isCancelled else { return }
                observeMessageTimes(messages)
                if notificationRooms.contains(id) { notifyCompletedReplies(messages, room: id) }
                guard let state = rooms[id], state.loaded else { continue }
                let decisions = try await client.participation(room: id)
                state.markers = try await client.markers(room: id)
                state.applyParticipation(decisions, conversation: id)
                if state.runs.isEmpty { state.bubbles = messages.map(ChatBubble.init) }
                else {
                    // Keep active stream content intact; refresh only Rust's derived eligibility.
                    let eligibility = Dictionary(uniqueKeysWithValues: messages.map { ($0.id, $0.canRetry) })
                    for index in state.bubbles.indices {
                        if let canRetry = eligibility[state.bubbles[index].id] { state.bubbles[index].canRetry = canRetry }
                    }
                }
            }
            catch { rooms[id]?.error = describe(error) }
        }
    }
    private func startMaintenanceObservation() {
        guard maintenanceObservation == nil else { return }
        maintenanceObservation = Task { @MainActor [weak self, client] in
            while !Task.isCancelled {
                let personas = self?.workspace?.personas ?? []
                for persona in personas {
                    guard let status = try? await client.maintenanceStatus(persona: persona.id) else { continue }
                    let revision = "\(status.id):\(status.status)"
                    guard self?.observedMaintenance[persona.id] != revision else { continue }
                    self?.observedMaintenance[persona.id] = revision
                    if ["failed", "cancelled", "interrupted"].contains(status.status) {
                        self?.room(persona.conversationId).error = status.detail
                        if self?.selectedConversation?.members.contains(where: { $0.personaId == persona.id }) == true {
                            self?.selectedRoom?.error = status.detail
                        }
                    }
                }
                do { try await Task.sleep(for: .seconds(1)) } catch { return }
            }
        }
    }
    func closeChat() {
        activityObservation?.cancel(); activityObservation = nil
        maintenanceObservation?.cancel(); maintenanceObservation = nil
        imageGenerationStream?.cancel(); imageGenerationStream = nil; imageGenerationSink?.finish(); imageGenerationSink = nil
        delegationStream?.cancel(); delegationStream = nil; delegationSink?.finish(); delegationSink = nil
        selectionRevision = UUID()
        maintenanceHandles.values.forEach { $0.cancel() }
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
        case .Busy: return "Wait for pending replies and participation checks to finish first."
        case .UnknownPersona: return "This character is no longer available. Reload the character files."
        case .EmptyMessage: return "Write something before sending."
        case let .Generation(detail): return "Couldn't start the reply. \(detail)"
        }
    }
}
