import AppKit

@MainActor
final class RecordingReplyNotifications: ReplyNotificationSink {
    var chatIsFocused = false
    var authorizationRequests = 0
    var replies: [ReplyNotification] = []
    func requestAuthorization() { authorizationRequests += 1 }
    func deliver(_ reply: ReplyNotification) { replies.append(reply) }
}

@MainActor
enum ReplyNotificationChecks {
    static func run(_ root: URL) async throws {
        let sink = RecordingReplyNotifications()
        let notifications = ReplyNotifications(sink: sink)
        let model = ChatModel(dataRoot: root, notifications: notifications)
        await model.load()
        let first = model.selected!
        try require(sink.authorizationRequests == 0 && sink.replies.isEmpty, "Loading requested notification permission or announced history")

        model.sendMessage("A private reply")
        try await wait { !model.hasActiveReplies }
        try require(sink.authorizationRequests == 1 && sink.replies.count == 1, "First send did not request once and announce its accepted reply")
        try require(sink.replies[0].roomID == first.conversationId && sink.replies[0].title == first.displayName,
                    "Private notification lost its room or character")
        let firstReply = model.bubbles.last!.id
        await model.load()
        await model.refreshRoomsAfterReply(notificationRooms: [first.conversationId])
        try require(sink.replies.count == 1, "Reload or task refresh announced an old reply")

        sink.chatIsFocused = true
        model.sendMessage("Read while focused")
        try await wait { !model.hasActiveReplies }
        sink.chatIsFocused = false
        await model.refreshRoomsAfterReply(notificationRooms: [first.conversationId])
        try require(sink.replies.count == 1 && sink.authorizationRequests == 1, "Focused reply was announced later or authorization repeated")

        // An unselected room has saved history but has never had a RoomState cache.
        let other = try await model.client.create(name: "Background character")
        let historical = try await acceptReply(model.client, room: other.conversationId, prompt: "Old report")
        await model.load()
        try require(model.selectedID == first.conversationId, "Preparing report history changed selection")
        model.delegationSink?.yield([task(other)])
        try await Task.sleep(for: .milliseconds(100))
        try require(sink.replies.count == 1, "Initial task snapshot flooded historical reports")
        let report = try await acceptReply(model.client, room: other.conversationId, prompt: "New saved report")
        model.delegationSink?.yield([task(other)])
        try await wait { sink.replies.count == 2 }
        try require(sink.replies.last?.messageID == report.id && sink.replies.last?.roomID == other.conversationId,
                    "An unloaded background report was missing or routed to selection")
        model.delegationSink?.yield([task(other)])
        await model.refreshRoomsAfterReply(notificationRooms: [other.conversationId])
        try require(sink.replies.count == 2, "Repeated report snapshots delivered duplicates")

        let grouped = await model.createGroup("Notification group", members: [first.id, other.id])
        try require(grouped, "Could not create notification group")
        try await wait { !model.loading }
        var classifier = try await model.readClassifierSettings(nil)
        classifier.connection.enabled = false
        try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
        let group = model.selectedConversation!
        model.mentionedIDs = [first.id, other.id]
        model.draft = "Both characters reply"
        model.sendMessage()
        model.select(first.id)
        try await wait { !model.hasActiveReplies && sink.replies.count == 4 }
        try require(sink.replies.suffix(2).allSatisfy { $0.roomID == group.id && $0.subtitle == group.title },
                    "Group replies followed selected private room or lost group title")

        // A task report in a group uses the same snapshot path, with no chat-owned handle.
        let groupReport = try await acceptReply(model.client, room: group.id, prompt: "Saved group report", mentions: [first.id])
        model.delegationSink?.yield([task(first, room: group.id)])
        try await wait { sink.replies.count == 5 }
        try require(sink.replies.last?.messageID == groupReport.id && sink.replies.last?.subtitle == group.title,
                    "Group task report was not announced in its original room")

        try checkFiltering(room: group, sample: historical, firstReply: firstReply)
        model.openNotificationRoom(group.id)
        try await wait { model.selectedID == group.id && !model.loading }
        try require(model.bubbles.contains { $0.id == groupReport.id }, "Notification navigation did not load the target transcript")
        model.openNotificationRoom("missing-room")
        try require(model.selectedID == group.id, "Stale notification selected a nonexistent room")
        model.closeChat()
        try await checkPendingRoute(root.appendingPathComponent("launch"))
        try checkNativeFocus()
        print("Reply notification checks passed: permission trigger, accepted private/group replies, focus suppression, history/dedup, unloaded report rooms and pending navigation.")
    }

    private static func checkFiltering(room: RoomRecord, sample: MessageRecord, firstReply: String) throws {
        let sink = RecordingReplyNotifications()
        let notifications = ReplyNotifications(sink: sink)
        func message(_ id: String, _ status: MessageStatus = .complete, text: String = "Accepted reply", sender: String = "persona") -> MessageRecord {
            MessageRecord(id: id, conversationId: room.id, sequence: 1, senderKind: sender,
                senderId: sample.senderId, content: text, status: status, createdAt: sample.createdAt,
                failureDetail: "", canRetry: false, images: [])
        }
        notifications.baseline([message("old"), message("queued", .queued)], room: room.id)
        notifications.baseline([message("gap")], room: room.id) // A reload must not reset the baseline.
        let accepted = [message("old"), message("queued"), message("gap")]
        notifications.completed(accepted, room: room, speakerName: { _ in "Character" })
        notifications.completed(accepted, room: room, speakerName: { _ in "Character" })
        try require(sink.replies.map(\.messageID) == ["queued", "gap"], "Placeholder completion or subscription-gap acceptance was lost, or history duplicated")
        notifications.completed([message(firstReply, .streaming), message("failed", .failed), message("cancelled", .cancelled),
            message("interrupted", .interrupted), message("empty", text: " \n"), message("user", sender: "user")],
            room: room, speakerName: { _ in "Character" })
        try require(sink.replies.count == 2, "A partial, unsuccessful, empty or user message was announced")
    }

    private static func acceptReply(_ client: CoreClient, room: String, prompt: String, mentions: [String] = []) async throws -> MessageRecord {
        let (stream, sink) = AsyncStream<ChatUpdate>.makeStream()
        let submission = try await client.send(room: room, prompt: prompt, mentions: mentions, observer: StreamObserver(sink))
        var accepted: MessageRecord?
        for await update in stream where update.kind == .completed {
            accepted = update.messages.first { $0.id == update.messageId && $0.status == .complete }
        }
        try require((!submission.handles.isEmpty || submission.group != nil) && accepted != nil,
                    "Synthetic report did not publish an accepted persona reply")
        return accepted!
    }

    private static func task(_ persona: PersonaRecord, room: String? = nil) -> DelegatedTaskRecord {
        DelegatedTaskRecord(id: "notification-fixture", title: "Saved task outcome", personaId: persona.id,
            conversationId: room ?? persona.conversationId, state: .completed, activity: "", detail: "", result: "",
            links: [], request: nil, reportState: "completed", threadId: nil, turnId: nil, createdAt: "", privateDelivery: false)
    }

    private static func checkPendingRoute(_ root: URL) async throws {
        let groupID: String
        do {
            let backend = try PebbleBackend(dataRoot: root.path)
            groupID = try backend.createGroup(title: "Launch destination", members: [backend.workspace().personas[0].id]).id
        }
        let model = ChatModel(dataRoot: root)
        model.openNotificationRoom(groupID) // Notification delegate can run before workspace loading.
        await model.load()
        try await wait { !model.loading && model.selectedID == groupID }
        try require(model.selectedConversation?.title == "Launch destination", "Launch-time room route was overwritten by default selection")
        model.closeChat()
    }

    private static func checkNativeFocus() throws {
        _ = NSApplication.shared
        let chat = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: true)
        let preview = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: true)
        let sheet = NSWindow(contentRect: .zero, styleMask: [.titled], backing: .buffered, defer: true)
        try require(NativeReplyNotificationSink.chatIsFocused(chat, keyWindow: chat, appIsActive: true), "Main chat focus was ignored")
        try require(!NativeReplyNotificationSink.chatIsFocused(chat, keyWindow: preview, appIsActive: true), "Image preview focus suppressed chat notifications")
        try require(!NativeReplyNotificationSink.chatIsFocused(chat, keyWindow: chat, appIsActive: false), "Inactive app retained chat focus")
        chat.beginSheet(sheet)
        try require(NativeReplyNotificationSink.chatIsFocused(chat, keyWindow: sheet, appIsActive: true), "Attached chat sheet was treated as another window")
        chat.endSheet(sheet)
        chat.orderOut(nil); preview.orderOut(nil); sheet.orderOut(nil)
    }

    private static func wait(line: UInt = #line, _ condition: () -> Bool) async throws {
        for _ in 0..<500 {
            if condition() { return }
            try await Task.sleep(for: .milliseconds(20))
        }
        try require(condition(), "Timed out waiting for reply notification state at line \(line)")
    }
}
