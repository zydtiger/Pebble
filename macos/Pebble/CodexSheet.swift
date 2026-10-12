import SwiftUI
import AppKit

struct CodexSheet: View {
    @Bindable var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var settings = CodexSettingsRecord(enabled: true, executable: "codex", workspace: "", writable: false, model: nil, effort: nil)
    @State private var modelName = ""
    @State private var effort = ""
    @State private var capabilities: CodexCapabilitiesRecord?
    @State private var detail = ""
    @State private var busy = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Codex tasks").font(.title2.weight(.semibold))
            Text("Characters hand external lookups and authorized work to your local Codex. Conversations stay usable while tasks run.").font(.system(size: 12)).foregroundStyle(.secondary)
            Form {
                Toggle("Enable Codex tasks", isOn: $settings.enabled)
                HStack {
                    TextField("Executable", text: $settings.executable)
                    Button("Choose…") { chooseExecutable() }
                }
                TextField("Model · inherit when empty", text: $modelName)
                Picker("Reasoning effort", selection: $effort) {
                    Text("Inherit local Codex configuration").tag("")
                    ForEach(["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"], id: \.self) { Text($0).tag($0) }
                }
                HStack {
                    TextField("Workspace · managed task folder when empty", text: $settings.workspace)
                    Button("Choose…") { chooseWorkspace() }
                }
                Toggle("Allow writes in the selected workspace", isOn: $settings.writable)
                Text("Read-only lookups use a private task folder by default. File/code writes require an explicit workspace. Native permissions are shown before Codex proceeds. Tasks are managed while Pebble is open; restart reconciles saved outcomes without resending work.").font(.system(size: 11)).foregroundStyle(.secondary)
            }
            if let capabilities {
                Text(capabilities.authenticated ? "Local Codex authentication is ready." : "Codex needs authentication. Run codex login, then check again.").font(.system(size: 12))
                Text("\(capabilities.models.count) models · \(capabilities.skills.count) enabled skills · \(capabilities.mcpServers.count) MCP servers").font(.system(size: 11)).foregroundStyle(.secondary)
                DisclosureGroup("Available capabilities") {
                    ScrollView { Text("Models: \(capabilities.models.joined(separator: ", "))\nSkills: \(capabilities.skills.joined(separator: ", "))\nMCP: \(capabilities.mcpServers.joined(separator: ", "))\n\n\(capabilities.detail)").font(.system(size: 10)).textSelection(.enabled) }.frame(maxHeight: 120)
                }
            }
            if !detail.isEmpty { Text(detail).font(.system(size: 11)).foregroundStyle(.secondary).textSelection(.enabled) }
            HStack {
                Button("Save & check connection") { save(check: true) }
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { save(check: false) }.keyboardShortcut(.defaultAction)
            }.disabled(busy)
        }.padding(24).frame(width: 610)
        .background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        .task {
            do { settings = try await model.client.codexSettings(); modelName = settings.model ?? ""; effort = settings.effort ?? "" }
            catch { detail = model.describe(error) }
        }
    }
    private func save(check: Bool) {
        busy = true; detail = ""
        settings.model = modelName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : modelName
        settings.effort = effort.isEmpty ? nil : effort
        Task {
            defer { busy = false }
            do { try await model.client.saveCodex(settings); if check { capabilities = try await model.client.codexCapabilities() } else { dismiss() } }
            catch { detail = model.describe(error) }
        }
    }
    private func chooseExecutable() {
        let panel = NSOpenPanel(); panel.canChooseDirectories = false; panel.canChooseFiles = true; panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url { settings.executable = url.path }
    }
    private func chooseWorkspace() {
        let panel = NSOpenPanel(); panel.canChooseDirectories = true; panel.canChooseFiles = false; panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url { settings.workspace = url.path }
    }
}
