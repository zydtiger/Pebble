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
    var roomTasks: [DelegatedTaskRecord] { delegatedTasks.filter { $0.conversationId == selectedID } }
    func taskActivity(_ room: String) -> Bool {
        delegatedTasks.contains { $0.conversationId == room && $0.state.active }
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
                await self.refreshRoomsAfterReply()
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
