import SwiftUI

struct ContentView: View {
    @Bindable var model: ChatModel
    @State private var composerFocused = true
    @State private var composerHeight = ComposerScrollView.minimumHeight

    var body: some View {
        HStack(spacing: 0) {
            sidebar
            Rectangle().fill(PebbleTheme.line.opacity(0.55)).frame(width: 1)
            VStack(spacing: 0) {
                if let persona = model.selected {
                    chatHeader(persona)
                    Rectangle().fill(PebbleTheme.line.opacity(0.55)).frame(height: 1)
                    if let error = model.error { errorBanner(error) }
                    if model.bubbles.isEmpty {
                        GeometryReader { geometry in
                            ScrollView {
                                introduction(persona)
                                    .frame(maxWidth: .infinity)
                                    .frame(minHeight: geometry.size.height)
                            }
                            .scrollIndicators(.never)
                        }
                    } else {
                        transcript(persona)
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
        .sheet(isPresented: $model.showCharacter) {
            if let persona = model.selected { CharacterSheet(model: model, persona: persona) }
        }
        .sheet(isPresented: $model.showNewCharacter) { NewCharacterSheet(model: model) }
        .task { await model.load() }
        .onDisappear { model.closeChat() }
        .onChange(of: model.selectedID) { _, _ in composerFocused = true }
    }

    private var sidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 9) {
                PebbleMark(size: 30)
                Text("Pebble").font(.system(size: 24, weight: .semibold, design: .rounded)).tracking(-0.8)
            }
            .padding(.top, 45).padding(.bottom, 8)
            Text("Small moments. Familiar voices.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                .padding(.bottom, 40)
            Text("YOUR PEOPLE")
                .font(.system(size: 10, weight: .semibold)).tracking(1.6)
                .foregroundStyle(PebbleTheme.muted).padding(.bottom, 13)
            ScrollView {
                VStack(spacing: 0) {
                    ForEach(model.workspace?.personas ?? [], id: \.id) { persona in
                        Button { model.select(persona.id) } label: {
                            HStack(spacing: 11) {
                                CharacterAvatar(size: 38)
                                VStack(alignment: .leading, spacing: 4) {
                                    Text(persona.displayName).font(.system(size: 13, weight: .semibold))
                                    Text(model.pendingCount(persona.id) > 0 ? "\(model.pendingCount(persona.id)) pending" : "Private chat").font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                }
                                Spacer(minLength: 0)
                                if persona.id == model.selectedID {
                                    Circle().fill(PebbleTheme.sage).frame(width: 5, height: 5)
                                }
                            }
                            .padding(11)
                            .background(persona.id == model.selectedID ? .white.opacity(0.72) : .clear,
                                        in: RoundedRectangle(cornerRadius: 14))
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("persona-\(persona.displayName)")
                    }
                }
            }.scrollIndicators(.hidden)
            Button { model.showNewCharacter = true } label: {
                Label("New character", systemImage: "plus").font(.system(size: 11))
            }.buttonStyle(.plain).foregroundStyle(PebbleTheme.muted).padding(.top, 14)
            Spacer()
            VStack(alignment: .leading, spacing: 8) {
                Text("Every friendship starts\nwith a small hello.")
                    .font(.system(size: 13, design: .serif)).italic()
                    .lineSpacing(4).foregroundStyle(PebbleTheme.muted)
                Rectangle().fill(PebbleTheme.line).frame(height: 1).padding(.vertical, 12)
                HStack {
                    Text("v0.0.4").font(.system(size: 10, design: .monospaced))
                    Spacer()
                    Button { model.openFiles() } label: { Image(systemName: "folder") }
                        .help("Open Pebble's data folder")
                    Button { Task { await model.load() } } label: { Image(systemName: "arrow.clockwise") }
                        .disabled(model.hasActiveReplies || model.loading)
                        .help("Reload character files")
                }
                .buttonStyle(.plain).foregroundStyle(PebbleTheme.muted)
            }
            .padding(.bottom, 23)
        }
        .padding(.horizontal, 21)
        .frame(width: 236)
        .frame(maxHeight: .infinity)
        .background(PebbleTheme.sidebar)
    }

    private func chatHeader(_ persona: PersonaRecord) -> some View {
        HStack(spacing: 12) {
            CharacterAvatar(size: 35)
            VStack(alignment: .leading, spacing: 3) {
                Text(persona.displayName).font(.system(size: 14, weight: .semibold))
                Text("Just the two of you").font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            }
            Spacer()
            Text(persona.connectionLabel).font(.system(size: 9, weight: .semibold)).tracking(1.2)
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
                PebbleMark(size: 96)
            }
            .frame(height: 190).padding(.bottom, 24)
            Text("A small hello.")
                .font(.system(size: 34, weight: .regular, design: .serif)).tracking(-0.8)
                .padding(.bottom, 12)
            Text("Meet \(persona.displayName). \(persona.subtitle)")
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

    private func transcript(_ persona: PersonaRecord) -> some View {
        ScrollViewReader { reader in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 24) {
                    Text("SAVED ON THIS MAC").font(.system(size: 9, weight: .medium)).tracking(1.4)
                        .foregroundStyle(PebbleTheme.muted).frame(maxWidth: .infinity).padding(.vertical, 8)
                    ForEach(model.bubbles) { bubble in
                        HStack(alignment: .top, spacing: 10) {
                            if bubble.isUser { Spacer(minLength: 60) }
                            else { CharacterAvatar(size: 28).padding(.top, 20) }
                            VStack(alignment: bubble.isUser ? .trailing : .leading, spacing: 7) {
                                Text(bubble.isUser ? "YOU" : persona.displayName.uppercased())
                                    .font(.system(size: 9, weight: .semibold)).tracking(1)
                                    .foregroundStyle(PebbleTheme.muted)
                                if bubble.text.isEmpty && bubble.status == .streaming {
                                    ProgressView().controlSize(.small).padding(17)
                                } else {
                                    Text(bubble.status == .queued ? "Waiting for earlier reply…" : (bubble.text.isEmpty ? "No reply text." : bubble.text))
                                        .font(.system(size: 14)).lineSpacing(6)
                                        .textSelection(.enabled)
                                        .padding(.horizontal, 18).padding(.vertical, 15)
                                        .background(bubble.isUser ? PebbleTheme.sage.opacity(0.11) : .white.opacity(0.85),
                                                    in: RoundedRectangle(cornerRadius: 18))
                                }
                                if let status = bubble.statusLabel {
                                    HStack {
                                        Text(status).font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
                                        if bubble.status == .queued { Button("Cancel") { model.cancelReply(bubble.id) }.font(.system(size: 10)).buttonStyle(.plain) }
                                    }
                                }
                            }
                            if !bubble.isUser { Spacer(minLength: 28) }
                        }
                    }
                    Color.clear.frame(height: 1).id("latest")
                }
                .padding(.horizontal, 30).padding(.vertical, 20)
            }
            .onAppear { reader.scrollTo("latest", anchor: .bottom) }
            .onChange(of: model.bubbles.count) { _, _ in reader.scrollTo("latest", anchor: .bottom) }
            .onChange(of: model.replyRevision) { _, _ in reader.scrollTo("latest", anchor: .bottom) }
        }
    }

    private var composer: some View {
        VStack(spacing: 11) {
            HStack(alignment: .bottom, spacing: 12) {
                MessageEditor(text: $model.draft, height: $composerHeight,
                              isFocused: $composerFocused, isEditable: !model.loading) {
                    model.sendMessage()
                }
                    .frame(height: composerHeight)
                if model.isReplying {
                    Button { model.stopReply() } label: { Image(systemName: "stop.fill").font(.system(size: 12)) }
                        .buttonStyle(.plain).foregroundStyle(PebbleTheme.sage)
                        .help("Stop this character's current reply").accessibilityLabel("Stop reply")
                }
                Button { model.sendMessage() } label: {
                    Image(systemName: "arrow.up").font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(.white).frame(width: 33, height: 33)
                        .background(PebbleTheme.sage, in: Circle())
                }.buttonStyle(.plain)
                    .disabled(model.loading || model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                    .help(model.isReplying ? "Queue message (Enter)" : "Send message (Enter)")
                    .accessibilityLabel(model.isReplying ? "Queue message" : "Send message")
            }
            .padding(.horizontal, 14).padding(.vertical, 12)
            .background(.white.opacity(0.8), in: RoundedRectangle(cornerRadius: 17))
            .overlay(RoundedRectangle(cornerRadius: 17).stroke(PebbleTheme.line.opacity(0.7), lineWidth: 1))
            Text(model.selected.map { $0.contextNotice.isEmpty ? "\($0.connectionLabel) · Messages saved on this Mac" : $0.contextNotice } ?? "Messages saved on this Mac")
                .font(.system(size: 10)).foregroundStyle(PebbleTheme.muted)
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
                CharacterAvatar(size: 50)
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
            Text("Edit SOUL.md and MEMORY.md in the character folder, then reload. Memory is maintained by hand in this version.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
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
