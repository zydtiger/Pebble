import Foundation

enum MembershipCheckError: Error { case failed(String) }
@MainActor
enum MembershipChecks {
    static func check(_ condition: @autoclosure () -> Bool, _ message: String) throws {
        if !condition() { throw MembershipCheckError.failed(message) }
    }
    static func wait(_ model: ChatModel, until condition: () -> Bool) async throws {
        for _ in 0..<700 { if condition() { return }; try await Task.sleep(for: .milliseconds(10)) }
        throw MembershipCheckError.failed("Membership lifecycle timed out: \(model.error ?? "no error")")
    }
    static func run(_ model: ChatModel, phase: (String) async throws -> Void = { _ in }) async throws {
        var classifier = try await model.readClassifierSettings(nil)
        classifier.connection.enabled = false
        try await model.saveClassifierSettings(nil, update: classifier, credential: .keep)
        let a = model.selected!.id
        let created = await model.createCharacter("Kai")
        try check(created, "Create member failed")
        try await wait(model, until: { !model.loading })
        let b = model.selected!.id
        let grouped = await model.createGroup("A place to return", members: [a, b])
        try check(grouped, "Create membership group failed")
        try await wait(model, until: { !model.loading })
        let room = model.selectedConversation!.id
        let originalInterval = model.selectedConversation!.members.first { $0.personaId == a }!.membershipId

        // These edits happen before the async acceptance task resumes on MainActor.
        model.draft = "Same text restored while accepting"
        model.mentionedIDs = [a]
        model.sendMessage()
        model.draft = "A different edit"; model.mentionedIDs = [b]
        model.draft = "Same text restored while accepting"; model.mentionedIDs = [a]
        model.select(b)
        model.draft = "Kai's private draft"
        try await wait(model, until: { !model.hasActiveReplies && !model.loading })
        model.select(room)
        try await wait(model, until: { !model.loading })
        try check(model.draft == "Same text restored while accepting" && model.mentionedIDs == [a], "Acceptance erased same-text/mention re-edit")
        try check(model.bubbles.count == 2, "Acceptance duplicated canonical input or reply")
        model.sendMessage()
        model.draft = "New text during the earlier reply"; model.mentionedIDs = [a, b]
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.draft == "New text during the earlier reply" && model.mentionedIDs == [a, b], "Earlier reply erased a newer edit")
        let ids = model.bubbles.map(\.id)
        try await phase("group-members-current")
        try await model.removeGroupMember(a, room: room)
        try check(model.mentionedIDs == [b] && model.draft == "New text during the earlier reply", "Removal did not prune only the removed chosen target")
        try check(model.selectedConversation!.formerMemberIds.contains(a), "Former member was not exposed for Rejoin")
        try await phase("group-members-removed")
        try await model.addGroupMember(a, room: room)
        try check(model.selectedConversation!.members.first { $0.personaId == a }!.membershipId != originalInterval, "Rejoin reused the old interval")
        try check(model.mentionedIDs == [b] && model.bubbles.map(\.id) == ids, "Rejoin retargeted mentions or changed transcript")
        var settings = try await model.readSettings(b)
        settings.persona!.displayName = "Kai Renamed"
        try await model.saveSettings(b, update: SettingsUpdateRecord(chat: settings.chat, credential: .keep, contextWindowTokens: settings.contextWindowTokens, maxOutputTokens: settings.maxOutputTokens, persona: settings.persona))
        try check(model.mentionedIDs == [b] && model.memberLabel(b) == "Kai Renamed", "Rename retargeted chosen mention IDs")
        try await phase("group-members-rejoined")
        model.select(b); try check(model.draft == "Kai's private draft", "Navigation lost private draft")
        model.select(room)
        model.mentionedIDs = []
        model.draft = "@MissingMember rejected input"
        model.sendMessage()
        model.draft = "Changed after failing send"
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.draft == "Changed after failing send" && model.error != nil, "Failed acceptance erased the newer draft")
        model.error = nil

        model.select(a)
        for index in 0..<3 { model.sendMessage("Independent private work \(index)") }
        model.select(room); model.mentionedIDs = [a, b]
        model.sendMessage("Remove waiting member, let the other finish")
        try await wait(model, until: { model.bubbles.contains { $0.senderID == a && $0.status == .queued } })
        try await model.removeGroupMember(a, room: room)
        try check(model.pendingCount(a) > 0, "Removal cancelled independent private work")
        try await phase("group-member-work-stopped")
        try await wait(model, until: { !model.hasActiveReplies })
        try check(model.bubbles.last?.senderID == b && model.bubbles.last?.status == .complete, "Removal stranded the other required member")
        try check(model.bubbles.contains { $0.senderID == a && $0.status == .cancelled && !$0.canRetry }, "Old interval exposed Retry after removal")
        try await model.removeGroupMember(b, room: room)
        try check(model.selectedConversation!.members.isEmpty, "Removing final member destroyed or retained roster")
        try await phase("group-members-empty")
        try await model.addGroupMember(b, room: room)
        model.closeChat()
        print("Native membership passed: future intervals, remove/rejoin, stable rename IDs, shared private work, same-text re-edit and per-room draft/mention preservation.")
    }
}
