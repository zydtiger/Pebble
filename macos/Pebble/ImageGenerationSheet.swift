import SwiftUI

struct ImageGenerationSheet: View {
    @Bindable var model: ChatModel
    @Environment(\.dismiss) private var dismiss
    @State private var studioEnabled = false
    @State private var chatgptEnabled = false
    @State private var baseURL = ""
    @State private var modelID = ""
    @State private var gpuID = ""
    @State private var steps = ""
    @State private var guidance = ""
    @State private var controllerModel = ""
    @State private var effort = ""
    @State private var detail = ""
    @State private var busy = false
    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text("Image generation").font(.title2.weight(.semibold))
            Form {
                Section("Studio") {
                    Toggle("Enable Studio", isOn: $studioEnabled)
                    if studioEnabled {
                        TextField("Base URL", text: $baseURL, prompt: Text("https://api.example.com"))
                        TextField("Model registration ID", text: $modelID)
                        TextField("GPU ID", text: $gpuID)
                        TextField("Steps", text: $steps, prompt: Text("Use server default"))
                        TextField("Guidance", text: $guidance, prompt: Text("Use server default"))
                    }
                }
                Section("ChatGPT") {
                    Toggle("Enable ChatGPT images", isOn: $chatgptEnabled)
                    if chatgptEnabled {
                        TextField("Model", text: $controllerModel, prompt: Text("Inherit when empty"))
                        Picker("Reasoning effort", selection: $effort) {
                            Text("Inherit local Codex").tag("")
                            ForEach(["none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra"], id: \.self) { Text($0).tag($0) }
                        }
                        Text("Uses your local Codex executable and ChatGPT login. Requested dimensions are best effort; the character reports the actual size.")
                            .font(.system(size: 11)).foregroundStyle(.secondary)
                    }
                }
            }
            if !detail.isEmpty { Text(detail).font(.system(size: 11)).foregroundStyle(.secondary).textSelection(.enabled) }
            HStack {
                if busy { ProgressView().controlSize(.small) }
                Spacer()
                Button("Cancel") { dismiss() }.keyboardShortcut(.cancelAction)
                Button("Save") { save() }.keyboardShortcut(.defaultAction)
            }.disabled(busy)
        }.padding(24).frame(width: 570)
        .background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        .task {
            do {
                let settings = try await model.client.imageSettings()
                studioEnabled = settings.studio != nil; chatgptEnabled = settings.chatgpt != nil
                if let s = settings.studio {
                    baseURL = s.baseUrl; modelID = s.modelId; gpuID = s.gpuId
                    steps = s.steps.map { String($0) } ?? ""; guidance = s.guidance.map { String($0) } ?? ""
                }
                controllerModel = settings.chatgpt?.model ?? ""; effort = settings.chatgpt?.effort ?? ""
            } catch { detail = model.describe(error) }
        }
    }
    private func save() {
        if studioEnabled && ((!steps.isEmpty && UInt32(steps) == nil) || (!guidance.isEmpty && Double(guidance) == nil)) {
            detail = "Enter valid numeric inference overrides, or leave them empty for server defaults."; return
        }
        let studio = studioEnabled ? StudioSettingsRecord(baseUrl: baseURL, modelId: modelID, gpuId: gpuID, steps: UInt32(steps), guidance: Double(guidance)) : nil
        let chatgpt = chatgptEnabled ? ChatGptImageSettingsRecord(model: controllerModel.isEmpty ? nil : controllerModel, effort: effort.isEmpty ? nil : effort) : nil
        busy = true
        Task {
            defer { busy = false }
            do { try await model.client.saveImageSettings(ImageGenerationSettingsRecord(studio: studio, chatgpt: chatgpt)); dismiss() }
            catch { detail = model.describe(error) }
        }
    }
}
