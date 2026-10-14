import SwiftUI

struct StateSheet: View {
    @Bindable var model: ChatModel
    let personaID: String
    @Environment(\.dismiss) private var dismiss
    @State private var snapshot: ResponseSettingsRecord?
    @State private var schema = ""
    @State private var initial = ""
    @State private var tab = "state"
    @State private var choice = "defaults"
    @State private var loading = true
    @State private var saving = false
    @State private var confirm = false
    @State private var error: String?
    private var name: String { model.workspace?.personas.first { $0.id == personaID }?.displayName ?? "Character" }

    init(model: ChatModel, personaID: String, initialTab: String = "state") {
        self.model = model
        self.personaID = personaID
        _tab = State(initialValue: initialTab)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                VStack(alignment: .leading, spacing: 4) {
                    Text("State & response").font(.system(size: 24, design: .serif))
                    Text(name).font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
                }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction).disabled(saving)
                Button(saving ? "Applying…" : "Apply") {
                    if choice == "defaults" { Task { await save() } } else { confirm = true }
                }.keyboardShortcut(.defaultAction).disabled(snapshot == nil || loading || saving)
            }
            if loading { ProgressView() }
            if let error { Text(error).font(.system(size: 12)).foregroundStyle(.red).textSelection(.enabled) }
            if let snapshot, snapshot.sessionId != nil && snapshot.activeSchemaJson != snapshot.schemaJson {
                Text("The current context keeps its earlier response contract. Choose a fresh context below to apply this schema; earlier messages remain saved.")
                    .font(.system(size: 12)).foregroundStyle(PebbleTheme.muted)
            }
            Picker("Editor", selection: $tab) {
                Text("State").tag("state")
                Text("Response schema").tag("schema")
            }.pickerStyle(.segmented)
            if tab == "state" {
                Text("Current accepted state").font(.system(size: 13, weight: .semibold))
                Text("Read only. Updated after a validated reply; independent of model context.")
                    .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                ScrollView {
                    Text(snapshot?.currentStateJson ?? "Loading…")
                        .font(.system(size: 12, design: .monospaced)).textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading).padding(12)
                }.frame(height: 120).background(.white.opacity(0.65), in: RoundedRectangle(cornerRadius: 8))
                Text("Initial defaults · JSON").font(.system(size: 13, weight: .semibold))
                Text("Saving defaults alone keeps the accepted state. They are used for new state only when you explicitly reset. Values must also fit in TOML.")
                    .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                jsonEditor($initial)
            } else {
                Text("Response schema · JSON Schema").font(.system(size: 13, weight: .semibold))
                Text("Define state fields, bounds, and descriptions. Keep the required reply + state envelope. A schema change needs a fresh context and a keep/reset choice.")
                    .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                jsonEditor($schema)
            }
            Divider()
            Picker("Apply with", selection: $choice) {
                Text("Save initial defaults only · keep state and context").tag("defaults")
                Text("Fresh context · keep compatible accepted state").tag("keep")
                Text("Fresh context · reset state to these initial defaults").tag("reset")
            }
            Text(choice == "defaults" ? "Schema edits require one of the fresh-context choices. Wait for this character’s pending replies to finish before applying." : "Earlier messages and native history stay saved, but are excluded from the new context. The current SOUL and MEMORY are loaded again.")
                .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            HStack {
                Button("Reload saved") { Task { await load() } }.disabled(saving)
                Button("Open character folder") {
                    if let persona = model.workspace?.personas.first(where: { $0.id == personaID }) {
                        NSWorkspace.shared.open(URL(fileURLWithPath: persona.directory))
                    }
                }
                Spacer()
            }
        }
        .padding(24).frame(width: 680, height: 720)
        .background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        .disabled(saving)
        .task { await load() }
        .confirmationDialog(choice == "reset" ? "Reset accepted state and start fresh?" : "Keep accepted state and start fresh?", isPresented: $confirm, titleVisibility: .visible) {
            Button(choice == "reset" ? "Reset state & start fresh" : "Keep state & start fresh", role: choice == "reset" ? .destructive : nil) { Task { await save() } }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text(choice == "reset" ? "This replaces the current accepted state with the initial defaults shown here and starts a new context. Earlier messages, calls, and native history remain saved." : "The current state must satisfy this schema. A new context loads this schema, SOUL, and MEMORY, and excludes earlier messages while keeping them saved.")
        }
    }

    private func jsonEditor(_ text: Binding<String>) -> some View {
        TextEditor(text: text).font(.system(size: 12, design: .monospaced))
            .scrollContentBackground(.hidden).padding(8).background(.white.opacity(0.65))
            .border(PebbleTheme.line).disabled(snapshot == nil || loading)
    }
    private func load() async {
        loading = true; error = nil
        defer { loading = false }
        do {
            let value = try await model.readResponseSettings(personaID)
            snapshot = value; schema = value.schemaJson; initial = value.initialStateJson
            choice = "defaults"
        } catch { snapshot = nil; self.error = model.describe(error) }
    }
    private func save() async {
        guard let snapshot else { return }
        saving = true; error = nil
        defer { saving = false }
        let action: ResponseSettingsAction = choice == "reset" ? .freshContextResetState : (choice == "keep" ? .freshContextKeepState : .initialDefaultsOnly)
        do {
            try await model.saveResponseSettings(personaID, update: ResponseSettingsUpdateRecord(expected: snapshot, schemaJson: schema, initialStateJson: initial, action: action))
            dismiss()
        } catch { self.error = model.describe(error) }
    }
}
