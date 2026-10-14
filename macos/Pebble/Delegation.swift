import AppKit
import Foundation

extension CoreClient {
    func codexSettings() throws -> CodexSettingsRecord { try requireBackend().codexSettings() }
    func saveCodex(_ settings: CodexSettingsRecord) throws { try requireBackend().saveCodexSettings(settings: settings) }
    func codexCapabilities() throws -> CodexCapabilitiesRecord { try requireBackend().codexCapabilities() }
    func observeTasks(_ observer: DelegationObserver) throws { try requireBackend().observeDelegations(observer: observer) }
    func cancelTask(_ id: String) throws { try requireBackend().cancelDelegation(id: id) }
    func answerTask(_ id: String, request: String, approve: Bool?, answers: [TaskAnswerRecord]) throws {
        try requireBackend().answerDelegation(id: id, request: request, approve: approve, answers: answers)
    }
    func followupTask(_ id: String, prompt: String) throws { try requireBackend().followupDelegation(id: id, prompt: prompt) }
    func reconcileTask(_ id: String) throws { try requireBackend().reconcileDelegation(id: id) }
    func retryTaskReport(_ id: String) throws { try requireBackend().retryDelegationReport(id: id) }
}
final class TaskObserver: DelegationObserver {
    let sink: AsyncStream<[DelegatedTaskRecord]>.Continuation
    init(_ sink: AsyncStream<[DelegatedTaskRecord]>.Continuation) { self.sink = sink }
    func onTasks(tasks: [DelegatedTaskRecord]) throws {
        if case .terminated = sink.yield(tasks) { throw ListenerError.Closed(detail: "The task view closed.") }
    }
}
extension DelegationState {
    var active: Bool { [.queued, .running, .needsInput, .needsApproval, .cancellationRequested].contains(self) }
    var label: String {
        switch self {
        case .queued: return "Queued"
        case .running: return "Working"
        case .needsInput: return "Needs your answer"
        case .needsApproval: return "Needs permission"
        case .cancellationRequested: return "Cancelling"
        case .completed: return "Completed"
        case .failed: return "Failed"
        case .cancelled: return "Cancelled"
        case .interrupted: return "Interrupted"
        case .uncertain: return "Outcome uncertain"
        }
    }
}
extension ChatModel {
    var monitorTasks: [MonitorTask] {
        let codex = delegatedTasks.reversed().filter {
            [.running, .cancellationRequested].contains($0.state) && $0.request == nil
        }.map { MonitorTask(id: $0.id, title: $0.title, personaId: $0.personaId, status: $0.state.label) }
        let images = imageGenerations.reversed().filter { $0.active }.map {
            MonitorTask(id: $0.id, title: "\($0.kind == .edit ? "Edit image" : "Generate image") · \($0.backend == "chatgpt" ? "ChatGPT" : "Studio")", personaId: $0.personaId, status: $0.activity.isEmpty ? $0.status.capitalized : $0.activity)
        }
        return codex + images
    }
    func taskActivity(_ room: String) -> Bool {
        delegatedTasks.contains { $0.conversationId == room && $0.state.active } || imageGenerations.contains { $0.conversationId == room && $0.active }
    }
    func startTaskObservation() async throws {
        guard delegationStream == nil else { return }
        let (stream, sink) = AsyncStream<[DelegatedTaskRecord]>.makeStream(bufferingPolicy: .bufferingNewest(1))
        delegationSink = sink
        try await client.observeTasks(TaskObserver(sink))
        delegationStream = Task { [weak self] in
            for await tasks in stream {
                guard let self, !Task.isCancelled else { break }
                self.delegatedTasks = tasks
                // Reports have no UI-owned chat handle. Scan their original rooms even
                // when no presentation cache exists; shared message IDs deduplicate streams.
                let reportRooms = self.replyNotifications == nil ? Set<String>() : Set(tasks.map(\.conversationId))
                await self.refreshRoomsAfterReply(notificationRooms: reportRooms)
            }
        }
    }
    func cancelTask(_ task: DelegatedTaskRecord) { performTaskAction { try await self.client.cancelTask(task.id) } }
    func reconcileTask(_ task: DelegatedTaskRecord) { performTaskAction { try await self.client.reconcileTask(task.id) } }
    func retryTaskReport(_ task: DelegatedTaskRecord) { performTaskAction { try await self.client.retryTaskReport(task.id) } }
    func followupTask(_ task: DelegatedTaskRecord, prompt: String) { performTaskAction { try await self.client.followupTask(task.id, prompt: prompt) } }
    func answerTask(_ task: DelegatedTaskRecord, approve: Bool?, answers: [TaskAnswerRecord]) {
        guard let request = task.request else { return }
        performTaskAction { try await self.client.answerTask(task.id, request: request.id, approve: approve, answers: answers) }
    }
    private func performTaskAction(_ action: @escaping @MainActor () async throws -> Void) {
        Task { do { try await action() } catch { self.error = self.describe(error) } }
    }
    func openTaskLink(_ link: TaskLinkRecord) {
        let url: URL?
        if link.target.hasPrefix("/") { url = URL(fileURLWithPath: link.target) }
        else if let parsed = URL(string: link.target), ["https", "http"].contains(parsed.scheme) { url = parsed }
        else { url = nil }
        if let url { NSWorkspace.shared.open(url) }
    }
}

struct MonitorTask {
    let id: String
    let title: String
    let personaId: String
    let status: String
}
extension CoreClient {
    func imageSettings() throws -> ImageGenerationSettingsRecord { try requireBackend().imageGenerationSettings() }
    func saveImageSettings(_ settings: ImageGenerationSettingsRecord) throws { try requireBackend().saveImageGenerationSettings(settings: settings) }
    func observeImages(_ observer: ImageGenerationObserver) throws { try requireBackend().observeImageGenerations(observer: observer) }
}
final class ImageObserver: ImageGenerationObserver {
    let sink: AsyncStream<[ImageGenerationRecord]>.Continuation
    init(_ sink: AsyncStream<[ImageGenerationRecord]>.Continuation) { self.sink = sink }
    func onImages(operations: [ImageGenerationRecord]) throws {
        if case .terminated = sink.yield(operations) { throw ListenerError.Closed(detail: "The image view closed.") }
    }
}
extension ImageGenerationRecord {
    var active: Bool { ["queued", "loading", "running", "cancelling"].contains(status) }
}
extension ChatModel {
    func startImageObservation() async throws {
        guard imageGenerationStream == nil else { return }
        let (stream, sink) = AsyncStream<[ImageGenerationRecord]>.makeStream(bufferingPolicy: .bufferingNewest(1))
        imageGenerationSink = sink
        try await client.observeImages(ImageObserver(sink))
        imageGenerationStream = Task { [weak self] in
            for await operations in stream {
                guard let self, !Task.isCancelled else { break }
                self.imageGenerations = operations
                let reportRooms = self.replyNotifications == nil ? Set<String>() : Set(operations.map(\.conversationId))
                await self.refreshRoomsAfterReply(notificationRooms: reportRooms)
            }
        }
    }
}
