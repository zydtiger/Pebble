import Foundation
import Observation

@MainActor
@Observable
final class RoomRun {
    let id = UUID()
    var handles: [ChatHandle] = []
    var pendingIDs: Set<String> = []
    var completedIDs: Set<String> = []
    var group: GroupHandle?
    var classifying = false
    var decisionIDs: Set<String> = []
    var accepting = true
    var acceptingPersonaIDs: Set<String> = []
    var task: Task<Void, Never>?
    var sink: AsyncStream<ChatUpdate>.Continuation?
    var cancelRequested = false
    func cancel() { cancelRequested = true; group?.cancel(); handles.forEach { $0.cancel() } }
}

@MainActor
@Observable
final class RoomState {
    var draft = "" { didSet { draftRevision += 1 } }
    var mentions: [String] = [] { didSet { mentionRevision += 1 } }
    var draftRevision = 0
    var mentionRevision = 0
    var bubbles: [ChatBubble] = []
    var markers: [ContextMarkerRecord] = []
    var participation: [ParticipationRecord] = []
    var error: String?
    var loaded = false
    var runs: [RoomRun] = []

    func applyParticipation(_ records: [ParticipationRecord], conversation: String) {
        for record in records where record.conversationId == conversation {
            if let index = participation.firstIndex(where: { $0.id == record.id }) { participation[index] = record }
            else { participation.append(record) }
        }
    }
    func apply(_ messages: [MessageRecord], conversation: String) {
        for message in messages where message.conversationId == conversation {
            if let index = bubbles.firstIndex(where: { $0.id == message.id }) { bubbles[index] = ChatBubble(message) }
            else { bubbles.append(ChatBubble(message)) }
        }
        bubbles.sort { $0.sequence < $1.sequence }
    }
    func consume(_ update: ChatUpdate, conversation: String) {
        apply(update.messages, conversation: conversation)
        if update.kind == .textDelta, let index = bubbles.firstIndex(where: { $0.id == update.messageId }) { bubbles[index].text += update.text }
        if update.kind == .failed { error = update.text }
    }
}
