import SwiftUI

struct GroupSheet: View {
    @Bindable var model: ChatModel
    var renaming: RoomRecord?
    @Environment(\.dismiss) private var dismiss
    @State private var title = ""
    @State private var members: Set<String> = []
    @State private var saving = false
    @State private var error: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(renaming == nil ? "A new group." : "Rename group.").font(.system(size: 28, design: .serif))
            TextField("Group name", text: $title).textFieldStyle(.roundedBorder)
            if renaming == nil {
                Text("Choose characters").font(.system(size: 12, weight: .semibold))
                ScrollView {
                    VStack(alignment: .leading, spacing: 14) {
                        ForEach(model.workspace?.personas ?? [], id: \.id) { persona in
                            Toggle(isOn: Binding(get: { members.contains(persona.id) }, set: { selected in
                                if selected { members.insert(persona.id) } else { members.remove(persona.id) }
                            })) {
                                VStack(alignment: .leading, spacing: 4) {
                                    Text((model.workspace?.personas.filter { $0.displayName == persona.displayName }.count ?? 0) > 1 ? "\(persona.displayName) · \(persona.id.prefix(6))" : persona.displayName).font(.system(size: 13, weight: .semibold))
                                    Text(persona.subtitle).font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
                                }
                            }.toggleStyle(.checkbox)
                        }
                    }.frame(maxWidth: .infinity, alignment: .leading).padding(2)
                }.frame(maxHeight: 240)
                Text("Characters share their own history and state across chats. Only public replies are shared with the group.")
                    .font(.system(size: 11)).foregroundStyle(PebbleTheme.muted)
            }
            if let error { Text(error).font(.system(size: 11)).foregroundStyle(.red).textSelection(.enabled) }
            HStack {
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Spacer()
                if saving { ProgressView().controlSize(.small) }
                Button(renaming == nil ? "Create group" : "Save name") {
                    saving = true
                    Task {
                        let result = if renaming == nil { await model.createGroup(title, members: (model.workspace?.personas ?? []).map(\.id).filter { members.contains($0) }) }
                                     else { await model.renameGroup(title) }
                        saving = false
                        if result { dismiss() } else { error = model.error }
                    }
                }.keyboardShortcut(.defaultAction)
                    .disabled(saving || title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || (renaming == nil && members.isEmpty))
            }
        }.padding(28).frame(width: 450).background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
            .onAppear { title = renaming?.title ?? "" }
    }
}
