import AppKit
import UserNotifications

struct ReplyNotification: Equatable {
    let messageID: String
    let roomID: String
    let title: String
    let subtitle: String
    let body: String
}

@MainActor
protocol ReplyNotificationSink: AnyObject {
    var chatIsFocused: Bool { get }
    func requestAuthorization()
    func deliver(_ reply: ReplyNotification)
}

/// Session-only bookkeeping: historical loads never become new-reply events.
@MainActor
final class ReplyNotifications {
    private let sink: any ReplyNotificationSink
    private var seen: Set<String> = []
    private var baselinedRooms: Set<String> = []
    private var requestedAuthorization = false

    init(sink: any ReplyNotificationSink) { self.sink = sink }

    func requestAuthorization() {
        guard !requestedAuthorization else { return }
        requestedAuthorization = true
        sink.requestAuthorization()
    }

    func needsBaseline(_ room: String) -> Bool { !baselinedRooms.contains(room) }

    func baseline(_ messages: [MessageRecord], room: String) {
        guard baselinedRooms.insert(room).inserted else { return }
        for message in messages where message.conversationId == room && message.status == .complete {
            seen.insert(message.id)
        }
    }

    func completed(_ messages: [MessageRecord], room: RoomRecord, speakerName: (String) -> String) {
        for message in messages where message.conversationId == room.id && message.senderKind == "persona"
            && message.status == .complete && !message.content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            // Remember focused replies too; later task refreshes must not announce them.
            guard seen.insert(message.id).inserted, !sink.chatIsFocused else { continue }
            sink.deliver(ReplyNotification(messageID: message.id, roomID: room.id,
                title: speakerName(message.senderId), subtitle: room.kind == "group" ? room.title : "",
                body: String(message.content.trimmingCharacters(in: .whitespacesAndNewlines).prefix(240))))
        }
    }
}

@MainActor
final class NativeReplyNotificationSink: NSObject, ReplyNotificationSink, UNUserNotificationCenterDelegate {
    private let center: UNUserNotificationCenter
    weak var chatWindow: NSWindow?
    var onOpenRoom: ((String) -> Void)?
    private var openWindow: (() -> Void)?
    private var pendingWindowOpen = false
    private var authorization: Task<Void, Never>?

    override init() {
        center = UNUserNotificationCenter.current()
        super.init()
        // Installed during App initialization, before launch finishes or a view loads.
        center.delegate = self
    }

    var chatIsFocused: Bool {
        Self.chatIsFocused(chatWindow, keyWindow: NSApp.keyWindow, appIsActive: NSApp.isActive)
    }

    static func chatIsFocused(_ chat: NSWindow?, keyWindow: NSWindow?, appIsActive: Bool) -> Bool {
        guard appIsActive, let chat else { return false }
        var candidate = keyWindow
        while let window = candidate {
            if window === chat { return true }
            candidate = window.sheetParent
        }
        return false
    }

    func requestAuthorization() {
        // macOS remembers denial; errors and denial leave ordinary chat unaffected.
        guard authorization == nil else { return }
        authorization = Task { _ = try? await center.requestAuthorization(options: [.alert, .sound]) }
    }

    func deliver(_ reply: ReplyNotification) {
        let content = UNMutableNotificationContent()
        content.title = reply.title
        content.subtitle = reply.subtitle
        content.body = reply.body
        content.sound = .default
        content.threadIdentifier = reply.roomID
        content.userInfo = ["roomID": reply.roomID]
        let request = UNNotificationRequest(identifier: reply.messageID, content: content, trigger: nil)
        Task {
            // A fast reply can finish while the first permission prompt is still open.
            // Only notification delivery waits; generation and chat stay responsive.
            await authorization?.value
            guard !chatIsFocused else { return }
            try? await center.add(request)
        }
    }

    func setWindowOpener(_ action: @escaping () -> Void) {
        openWindow = action
        if pendingWindowOpen { revealChat() }
    }

    private func revealChat() {
        pendingWindowOpen = openWindow == nil
        openWindow?()
        chatWindow?.deminiaturize(nil)
        chatWindow?.makeKeyAndOrderFront(nil)
        NSApp.activate(ignoringOtherApps: true)
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter,
        willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        // A reply can queue just before the user returns to the chat.
        await MainActor.run { self.chatIsFocused ? [] : [.banner, .list, .sound] }
    }

    nonisolated func userNotificationCenter(_ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse) async {
        guard response.actionIdentifier == UNNotificationDefaultActionIdentifier,
              let room = response.notification.request.content.userInfo["roomID"] as? String else { return }
        await MainActor.run {
            self.onOpenRoom?(room)
            self.revealChat()
        }
    }
}
