import SwiftUI

struct ContentView: View {
    @Bindable var model: ChatModel
    @State private var composerFocused = true
    @State private var composerHeight = ComposerScrollView.minimumHeight
    @State private var hoveredRoomID: String?

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            Rectangle().fill(PebbleTheme.line.opacity(0.55)).frame(width: 1)
            VStack(spacing: 0) {
                if let conversation = model.selectedConversation {
                    if let persona = model.selected { chatHeader(persona) } else { groupHeader(conversation) }
                    Rectangle().fill(PebbleTheme.line.opacity(0.55)).frame(height: 1)
                    if conversation.kind == "group" { ParticipationPanel(model: model) }
                    if let error = model.error { errorBanner(error) }
                    ZStack(alignment: .topTrailing) {
                        if model.bubbles.isEmpty && model.markers.isEmpty {
                            GeometryReader { geometry in
                                ScrollView {
                                    Group { if let persona = model.selected { introduction(persona) } else { groupIntroduction } }
                                        .frame(maxWidth: .infinity)
                                        .frame(minHeight: geometry.size.height)
                                }
                                .scrollIndicators(.never)
                            }
                        } else {
                            transcript
                        }
                        if !model.monitorTasks.isEmpty {
                            TaskMonitor(model: model).padding(.top, 12).padding(.trailing, 30)
                        }
                    }
                    composer
                } else {
                    unavailable
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(PebbleTheme.canvas)
        }
        .foregroundStyle(PebbleTheme.ink)
        .frame(minWidth: 820, minHeight: 610)
        .preferredColorScheme(.light)
        .sheet(isPresented: $model.showCodexSettings) { CodexSheet(model: model) }
        .sheet(isPresented: $model.showCharacter) {
            if let persona = model.selected { CharacterSheet(model: model, persona: persona) }
        }
        .sheet(item: $model.settingsTarget) { target in SettingsSheet(model: model, target: target) }
        .sheet(item: $model.classifierTarget) { target in ClassifierSheet(model: model, target: target) }
        .sheet(item: $model.stateTarget) { target in
            if let id = target.personaID { StateSheet(model: model, personaID: id) }
        }
        .sheet(isPresented: $model.showNewCharacter) { NewCharacterSheet(model: model) }
        .sheet(isPresented: $model.showNewGroup) { GroupSheet(model: model) }
        .sheet(isPresented: $model.showRenameGroup) { GroupSheet(model: model, renaming: model.selectedConversation) }
        .sheet(isPresented: $model.showGroupMembers) {
            if let room = model.selectedConversation { GroupMembersSheet(model: model, roomID: room.id) }
        }
        .confirmationDialog("Start a fresh context?", isPresented: $model.showFreshContext, titleVisibility: .visible) {
            Button("Start fresh context") { Task { await model.startFreshContext() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Remember important details, then start a fresh context. Messages and accepted character state stay saved. You can cancel; an unfinished attempt keeps the old context active, while memory already saved is preserved.")
        }
        .task { await model.load() }
        .onDisappear { model.closeChat() }
        .onChange(of: model.selectedID) { _, _ in composerFocused = true; model.showFreshContext = false }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 9) {
                PebbleMark(size: 30)
                Text("Pebble").font(.system(size: 24, weight: .semibold, design: .rounded)).tracking(-0.8)
            }
            .padding(.top, 22).padding(.bottom, 18)
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(model.workspace?.personas ?? [], id: \.id) { persona in
                        Button { model.select(persona.id) } label: {
                            HStack(spacing: 11) {
                                CharacterAvatar(size: 38, image: model.avatars[persona.id])
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(persona.displayName).font(.system(size: 13, weight: .semibold))
                                    Text(model.pendingCount(persona.id) > 0 ? "\(model.pendingCount(persona.id)) pending" : "Private chat").font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                }
                                Spacer(minLength: 0)
                                if model.taskActivity(persona.conversationId) { Image(systemName: "bolt.horizontal.circle").font(.system(size: 11)).foregroundStyle(PebbleTheme.sage) }
                                if persona.conversationId == model.selectedID {
                                    Circle().fill(PebbleTheme.sage).frame(width: 5, height: 5)
                                }
                            }
                            .padding(11)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .contentShape(Rectangle())
                            .background(sidebarRowBackground(persona.conversationId),
                                        in: RoundedRectangle(cornerRadius: 14))
                        }
                        .buttonStyle(.plain)
                        .onHover { hovering in hoveredRoomID = hovering ? persona.conversationId : nil }
                        .accessibilityIdentifier("persona-\(persona.displayName)")
                    }
                    if let groups = model.workspace?.rooms.filter({ $0.kind == "group" }), !groups.isEmpty {
                        Text("GROUP CHATS").font(.system(size: 10, weight: .semibold)).tracking(1.6)
                            .foregroundStyle(PebbleTheme.muted).frame(maxWidth: .infinity, alignment: .leading).padding(.top, 24).padding(.bottom, 9)
                        ForEach(groups, id: \.id) { group in
                            Button { model.select(group.id) } label: {
                                HStack(spacing: 11) {
                                    Image(systemName: "person.2").font(.system(size: 19)).foregroundStyle(PebbleTheme.sage).frame(width: 38, height: 38)
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(group.title).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                                        Text(model.pendingCount(group.id) > 0 ? "\(model.pendingCount(group.id)) pending" : "\(group.members.count) characters").font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                    }
                                    Spacer(minLength: 0)
                                    if model.taskActivity(group.id) { Image(systemName: "bolt.horizontal.circle").font(.system(size: 11)).foregroundStyle(PebbleTheme.sage) }
                                    if group.id == model.selectedID { Circle().fill(PebbleTheme.sage).frame(width: 5, height: 5) }
                                }
                                .padding(11)
                                .frame(maxWidth: .infinity, alignment: .leading)
                                .contentShape(Rectangle())
                                .background(sidebarRowBackground(group.id), in: RoundedRectangle(cornerRadius: 14))
                            }
                            .buttonStyle(.plain)
                            .onHover { hovering in hoveredRoomID = hovering ? group.id : nil }
                            .accessibilityIdentifier("group-\(group.title)")
                        }
                    }
                }
            }.scrollIndicators(.hidden)
            Button { model.showNewCharacter = true } label: {
                Label("New character", systemImage: "plus").font(.system(size: 11))
            }.buttonStyle(SidebarActionButtonStyle()).foregroundStyle(PebbleTheme.muted).padding(.top, 14)
            Button { model.showNewGroup = true } label: {
                Label("New group", systemImage: "person.2.badge.plus").font(.system(size: 11))
            }.buttonStyle(SidebarActionButtonStyle()).foregroundStyle(PebbleTheme.muted).padding(.top, 12)
            Spacer()
            VStack(alignment: .leading, spacing: 8) {
                Rectangle().fill(PebbleTheme.line).frame(height: 1).padding(.vertical, 12)
                HStack {
                    Text("v0.3.0").font(.system(size: 10, design: .monospaced))
                    Spacer()
                    Button { model.showCodexSettings = true } label: { Image(systemName: "bolt.horizontal.circle") }.help("Codex task settings")
                    Button { model.settingsTarget = SettingsTarget(personaID: nil) } label: { Image(systemName: "gearshape") }
                        .help("App defaults")
                    Button { model.openFiles() } label: { Image(systemName: "folder") }
                        .help("Open Pebble's data folder")
                    Button { Task { await model.load() } } label: { Image(systemName: "arrow.clockwise") }
                        .disabled(model.hasActiveReplies || model.loading)
                        .help("Reload character files")
                }
                .buttonStyle(SidebarActionButtonStyle()).foregroundStyle(PebbleTheme.muted)
            }
            .padding(.bottom, 23)
        }
        .padding(.horizontal, 21)
        .frame(width: 236)
        .frame(maxHeight: .infinity)
        .background(PebbleTheme.sidebar)
    }

    private func sidebarRowBackground(_ roomID: String) -> Color {
        if roomID == model.selectedID { return .white.opacity(0.72) }
        return roomID == hoveredRoomID ? .gray.opacity(0.12) : .clear
    }

    private func groupHeader(_ group: RoomRecord) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "person.2").font(.system(size: 25)).foregroundStyle(PebbleTheme.sage)
            VStack(alignment: .leading, spacing: 4) {
                Text(group.title).font(.system(size: 14, weight: .semibold))
                Text(group.members.isEmpty ? "No current members" : group.members.map(\.displayName).joined(separator: " · ")).font(.system(size: 11)).foregroundStyle(PebbleTheme.muted).lineLimit(2)
            }
            Spacer()
            Menu {
                ForEach(group.members, id: \.personaId) { member in
                    Button("Open \(model.memberLabel(member.personaId))'s private chat") { model.select(member.personaId) }
                }
            } label: { Image(systemName: "person.crop.circle") }.menuStyle(.borderlessButton).fixedSize().help("Character private chats and settings")
            Button { model.showGroupMembers = true } label: { Image(systemName: "person.2.badge.gearshape") }
                .buttonStyle(.plain).help("Manage group members").accessibilityIdentifier("manage-group-members")
            Button { model.showRenameGroup = true } label: { Image(systemName: "pencil") }
                .buttonStyle(.plain).help("Rename group")
        }.padding(.horizontal, 30).padding(.top, 30).padding(.bottom, 20)
    }
    private var groupIntroduction: some View {
        Image(systemName: "person.2").font(.system(size: 48, weight: .light)).foregroundStyle(PebbleTheme.sage)
            .frame(maxWidth: .infinity, minHeight: 300).padding(30)
    }

    private func chatHeader(_ persona: PersonaRecord) -> some View {
        HStack(spacing: 12) {
            if let image = model.avatars[persona.id] {
                Button {
                    ImagePreviewWindow.open(
                        sourcePath: URL(fileURLWithPath: persona.directory).appendingPathComponent("avatar.png").path,
                        title: persona.displayName,
                        unavailableMessage: "Character image is missing or corrupt.")
                } label: {
                    CharacterAvatar(size: 35, image: image).contentShape(Circle())
                }
                .buttonStyle(.plain)
                .help("View \(persona.displayName)'s picture")
                .accessibilityLabel("View \(persona.displayName)'s picture")
                .accessibilityIdentifier("character-avatar-preview")
            } else {
                CharacterAvatar(size: 35)
            }
            Text(persona.displayName).font(.system(size: 14, weight: .semibold))
            Spacer()
            Button { model.settingsTarget = SettingsTarget(personaID: persona.id) } label: { Image(systemName: "slider.horizontal.3") }
                .buttonStyle(.plain).foregroundStyle(PebbleTheme.muted)
                .help("Character settings")
                .disabled(model.isReplying || model.loading)
            Button { model.showFreshContext = true } label: { Image(systemName: "sparkles") }
                .buttonStyle(.plain).foregroundStyle(PebbleTheme.muted)
                .disabled(model.isReplying || model.loading || model.isMaintaining)
                .help("Review memory and start a fresh model context; keep saved messages")
                .accessibilityLabel("Start fresh context")
            Button { model.stateTarget = SettingsTarget(personaID: persona.id) } label: { Image(systemName: "curlybraces") }
                .buttonStyle(.plain).foregroundStyle(PebbleTheme.muted)
                .help("Character state and response schema")
                .accessibilityLabel("Character state and response schema")
                .disabled(model.loading)
            HStack(spacing: 6) {
                if persona.model != nil {
                    Circle().fill(PebbleTheme.sage).frame(width: 5, height: 5)
                        .accessibilityHidden(true)
                }
                Text(persona.model == nil ? "Local demo" : "Live")
                    .font(.system(size: 9, weight: .semibold)).tracking(1.2)
            }
            .foregroundStyle(PebbleTheme.sage)
            .padding(.horizontal, 10).padding(.vertical, 6)
            .background(PebbleTheme.sage.opacity(0.08), in: Capsule())
            Button { model.showCharacter = true } label: { Image(systemName: "info.circle").font(.system(size: 17)) }
                .buttonStyle(.plain).foregroundStyle(PebbleTheme.muted)
                .padding(.leading, 10).help("About \(persona.displayName)")
                .accessibilityIdentifier("character-info")
        }
        .padding(.horizontal, 30).padding(.top, 30).padding(.bottom, 20)
    }

    private func introduction(_ persona: PersonaRecord) -> some View {
        VStack(spacing: 0) {
            Spacer(minLength: 20)
            ZStack {
                Circle().fill(PebbleTheme.sage.opacity(0.045)).frame(width: 154, height: 154)
                Circle().stroke(PebbleTheme.sage.opacity(0.075), lineWidth: 1).frame(width: 186, height: 186)
                if let image = model.avatars[persona.id] {
                    CharacterAvatar(size: 128, image: image)
                } else {
                    PebbleMark(size: 96)
                }
            }
            .frame(height: 190).padding(.bottom, 24)
            Text(persona.subtitle)
                .font(.system(size: 13)).foregroundStyle(PebbleTheme.muted)
                .multilineTextAlignment(.center).lineSpacing(5)
                .frame(maxWidth: 320).padding(.bottom, 28)
            HStack(spacing: 9) {
                Button("Say hello") { model.sendMessage("Hi, \(persona.displayName). Tell me a little about yourself.") }
                Button("Share a small thought") { model.sendMessage("What's something small worth noticing today?") }
            }
            .buttonStyle(SoftButtonStyle())
            .disabled(model.loading || model.isReplying)
            Spacer(minLength: 30)
        }
        .padding(.horizontal, 28)
    }

    private var transcript: some View {
        ScrollViewReader { reader in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 24) {
                    contextDividers(after: 0)
                    ForEach(model.bubbles) { bubble in
                        HStack(alignment: .top, spacing: 10) {
                            if bubble.isUser { Spacer(minLength: 60) }
                            else { CharacterAvatar(size: 28, image: model.avatars[bubble.senderID]).padding(.top, 20) }
                            VStack(alignment: bubble.isUser ? .trailing : .leading, spacing: 7) {
                                Text(bubble.isUser ? "YOU" : model.speakerName(bubble.senderID).uppercased())
                                    .font(.system(size: 9, weight: .semibold)).tracking(1)
                                    .foregroundStyle(PebbleTheme.muted)
                                if !bubble.images.isEmpty { ImageAttachments(images: bubble.images) }
                                if bubble.text.isEmpty && !bubble.images.isEmpty { EmptyView() }
                                else if bubble.text.isEmpty && bubble.status == .streaming {
                                    HStack(spacing: 9) {
                                        ProgressView().controlSize(.small)
                                        Text("Typing…").font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
                                    }.padding(17)
                                } else {
                                    messageText(bubble)
                                        .font(.system(size: 14)).lineSpacing(6)
                                        .textSelection(.enabled)
                                        .padding(.horizontal, 18).padding(.vertical, 15)
                                        .background(bubble.isUser ? PebbleTheme.sage.opacity(0.11) : .white.opacity(0.85),
                                                    in: RoundedRectangle(cornerRadius: 18))
                                }
                                if !bubble.failureDetail.isEmpty {
                                    Text(bubble.failureDetail).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                        .textSelection(.enabled)
                                }
                                if let status = bubble.statusLabel {
                                    HStack {
                                        Text(status).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                        if model.canRetry(bubble) {
                                            Button("Retry") { model.retryReply(bubble.id) }
                                                .font(.system(size: 10)).buttonStyle(.plain)
                                                .accessibilityIdentifier("retry-\(bubble.id)")
                                        }
                                        if bubble.status == .queued { Button("Cancel") { model.cancelReply(bubble.id) }.font(.system(size: 10)).buttonStyle(.plain) }
                                    }
                                }
                            }
                            if !bubble.isUser { Spacer(minLength: 28) }
                        }
                        contextDividers(after: bubble.sequence)
                    }
                    Color.clear.frame(height: 1).id("latest")
                }
                .padding(.horizontal, 30).padding(.bottom, 20)
                .padding(.top, model.monitorTasks.isEmpty ? 20 : 52)
            }
            .onAppear { reader.scrollTo("latest", anchor: .bottom) }
            .onChange(of: model.bubbles.count) { _, _ in reader.scrollTo("latest", anchor: .bottom) }
            .onChange(of: model.markers.count) { _, _ in reader.scrollTo("latest", anchor: .bottom) }
            .onChange(of: model.replyRevision) { _, _ in reader.scrollTo("latest", anchor: .bottom) }
        }
    }

    @ViewBuilder
    private func messageText(_ bubble: ChatBubble) -> some View {
        if bubble.status == .queued {
            Text("Waiting for earlier reply…")
        } else if bubble.text.isEmpty {
            Text("No reply text.")
        } else if bubble.isUser {
            Text(verbatim: bubble.text)
        } else {
            ReplyMarkdown(text: bubble.text)
        }
    }

    private func contextDividers(after sequence: Int64) -> some View {
        ForEach(model.markers.filter { $0.afterSequence == sequence }, id: \.id) { marker in
            Rectangle().fill(Color.gray.opacity(0.35)).frame(height: 0.5)
                .padding(.vertical, 5)
                .id("\(model.selectedID ?? ""):\(marker.id)")
                .help("\(marker.label). Only this character started fresh. Other characters may still finish earlier replies.")
                .accessibilityLabel(marker.label)
        }
    }
    private var composer: some View {
        VStack(alignment: .leading, spacing: 11) {
            if model.isMaintaining, let detail = model.maintenanceDetail {
                HStack(spacing: 8) {
                    ProgressView().controlSize(.small)
                    Text(detail).font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                    Button("Cancel") { model.cancelMaintenance() }.font(.system(size: 11))
                }.accessibilityIdentifier("memory-maintenance")
            }
            if let group = model.selectedConversation, group.kind == "group" {
                HStack(spacing: 8) {
                    Menu {
                        ForEach(group.members, id: \.personaId) { member in
                            Button { model.toggleMention(member.personaId) } label: {
                                Label(model.memberLabel(member.personaId), systemImage: model.mentionedIDs.contains(member.personaId) ? "checkmark.circle.fill" : "circle")
                            }
                        }
                        Divider()
                        Button { model.separateTasks = false } label: { Label("Shared delegated task", systemImage: model.separateTasks ? "circle" : "checkmark.circle.fill") }
                        Button { model.separateTasks = true } label: { Label("Separate tasks for mentioned characters", systemImage: model.separateTasks ? "checkmark.circle.fill" : "circle") }.disabled(model.mentionedIDs.count < 2)
                    } label: { Label(model.separateTasks && model.mentionedIDs.count > 1 ? "Mention · separate tasks" : "Mention", systemImage: "at").font(.system(size: 11)) }
                        .menuStyle(.borderlessButton).fixedSize().help("Choose who should reply")
                    ScrollView(.horizontal) {
                        HStack(spacing: 6) {
                            ForEach(model.mentionedIDs, id: \.self) { id in
                                Button { model.toggleMention(id) } label: {
                                    HStack(spacing: 5) { Text("@\(model.memberLabel(id))"); Image(systemName: "xmark").font(.system(size: 8)) }
                                        .font(.system(size: 11)).padding(.horizontal, 9).padding(.vertical, 5)
                                        .background(PebbleTheme.sage.opacity(0.1), in: Capsule())
                                }.buttonStyle(.plain).help("Remove selected mention")
                            }
                        }
                    }.scrollIndicators(.hidden)
                }.foregroundStyle(PebbleTheme.sage)
            }
            if !model.draftImages.isEmpty { ImageAttachments(images: model.draftImages, remove: model.removeImage) }
            HStack(alignment: .bottom, spacing: 12) {
                Button { model.pickImages() } label: {
                    Image(systemName: "plus").font(.system(size: 14))
                        .frame(width: 33, height: 33).contentShape(Rectangle())
                }
                    .buttonStyle(.plain).help("Add images").accessibilityLabel("Add images").disabled(model.loading || model.importingImages)
                MessageEditor(text: $model.draft, height: $composerHeight,
                              isFocused: $composerFocused, isEditable: !model.loading,
                              onSubmit: { model.sendMessage() }, onImages: model.importImages,
                              onImageError: { model.error = $0 })
                    .frame(height: composerHeight)
                if model.isReplying {
                    Button { model.stopReply() } label: { Image(systemName: "stop.fill").font(.system(size: 12)) }
                        .buttonStyle(.plain).foregroundStyle(PebbleTheme.sage)
                        .help("Stop every pending reply and check in this chat").accessibilityLabel("Stop reply")
                }
                Button { model.sendMessage() } label: {
                    Image(systemName: "arrow.up").font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white).frame(width: 33, height: 33)
                        .background(PebbleTheme.sage, in: Circle())
                }.buttonStyle(.plain)
                    .disabled(model.loading || model.importingImages || (model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && model.mentionedIDs.isEmpty && model.draftImages.isEmpty))
                    .help(model.isReplying ? "Queue message (Enter)" : "Send message (Enter)")
                    .accessibilityLabel(model.isReplying ? "Queue message" : "Send message")
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .background(.white.opacity(0.8), in: RoundedRectangle(cornerRadius: 17))
            .overlay(RoundedRectangle(cornerRadius: 17).stroke(PebbleTheme.line.opacity(0.7), lineWidth: 1))
        }
        .padding(.horizontal, 30).padding(.bottom, 22).padding(.top, 12)
    }

    private func errorBanner(_ error: String) -> some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: "exclamationmark.circle")
            Text(error).font(.system(size: 12)).textSelection(.enabled)
            Spacer()
            Button { model.error = nil } label: { Image(systemName: "xmark") }.buttonStyle(.plain)
        }
        .foregroundStyle(Color(red: 0.57, green: 0.31, blue: 0.22))
        .padding(15).background(PebbleTheme.clay.opacity(0.12))
    }

    private var unavailable: some View {
        VStack(spacing: 18) {
            PebbleMark(size: 68)
            if model.loading {
                ProgressView("Opening your space…")
            } else {
                Text(model.error == nil ? "A space for someone new." : "Let's check your character files.")
                    .font(.system(size: 25, design: .serif))
                Text(model.error ?? "Add a character folder, then reload to meet them here.")
                    .font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
                    .textSelection(.enabled).multilineTextAlignment(.center).frame(maxWidth: 440)
                HStack {
                    Button("Open data folder") { model.openFiles() }
                    Button("Try again") { Task { await model.load() } }
                }.buttonStyle(SoftButtonStyle())
            }
        }.padding(35).frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct CharacterSheet: View {
    @Bindable var model: ChatModel
    let persona: PersonaRecord
    @Environment(\.dismiss) private var dismiss
    @State private var page = 0

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            HStack(spacing: 14) {
                CharacterAvatar(size: 50, image: model.avatars[persona.id])
                VStack(alignment: .leading, spacing: 5) {
                    Text(persona.displayName).font(.system(size: 25, design: .serif))
                    Text(persona.subtitle).font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
                }
                Spacer()
                Button("Done") { dismiss() }.keyboardShortcut(.cancelAction)
            }
            Picker("Character files", selection: $page) {
                Text("Character").tag(0)
                Text("Memory").tag(1)
            }.pickerStyle(.segmented)
            ScrollView {
                Text(page == 0 ? persona.soul : persona.memory)
                    .font(.system(size: 13)).lineSpacing(6).textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(18)
            }
            .frame(height: 230)
            .background(.white.opacity(0.8), in: RoundedRectangle(cornerRadius: 12))
            Text("With a chat model connected, useful details can be saved during chats and reviewed when you start fresh. You can also edit SOUL.md and MEMORY.md in the character folder, then reload.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            if !persona.contextNotice.isEmpty {
                Text(persona.contextNotice).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
            }
            HStack {
                Text(persona.connectionLabel)
                    .font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                Spacer()
                Button("Open character folder") { model.openCharacterFiles() }
            }
        }
        .padding(28).frame(width: 540)
        .background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
    }
}
