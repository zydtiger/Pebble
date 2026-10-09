import Foundation
import Observation

@MainActor
final class RoomRun {
    let id = UUID()
    var handle: ChatHandle?
    var task: Task<Void, Never>?
    var sink: AsyncStream<ChatUpdate>.Continuation?
    var cancelRequested = false
    func cancel() { cancelRequested = true; handle?.cancel() }
}

@MainActor
@Observable
final class RoomState {
    var draft = ""
    var bubbles: [ChatBubble] = []
    var error: String?
    var loaded = false
    var runs: [RoomRun] = []

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
