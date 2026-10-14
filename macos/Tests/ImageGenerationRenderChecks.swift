import AppKit

@MainActor
func checkImageGenerationRendering(window: NSWindow, model: ChatModel) async throws {
    try await model.client.saveImageSettings(ImageGenerationSettingsRecord(
        studio: StudioSettingsRecord(baseUrl: "https://api.example.com", modelId: "configured-registration-id", gpuId: "configured-gpu-id", steps: nil, guidance: nil),
        chatgpt: ChatGptImageSettingsRecord(model: nil, effort: nil)))
    model.showImageGenerationSettings = true
    try await Task.sleep(for: .milliseconds(400))
    guard let sheet = window.attachedSheet else { throw RenderError.failed("Image settings sheet did not open") }
    try RenderChecks.capture(sheet, name: "image-generation-settings")
    model.showImageGenerationSettings = false
    for _ in 0..<20 where window.attachedSheet != nil { try await Task.sleep(for: .milliseconds(50)) }
    guard window.attachedSheet == nil, let persona = model.selected else { throw RenderError.failed("Image settings did not close") }
    model.imageGenerationStream?.cancel(); model.imageGenerationStream = Task {}
    model.imageGenerationSink?.finish(); model.imageGenerationSink = nil
    model.imageGenerations = [ImageGenerationRecord(id: "render-image", personaId: persona.id, conversationId: persona.conversationId,
        kind: .generate, backend: "studio", status: "loading", activity: "Loading the Studio model…", detail: "", publicationStatus: "pending", assetId: nil, actualWidth: nil, actualHeight: nil)]
    try await Task.sleep(for: .milliseconds(200))
    guard let button = RenderChecks.findAccessible(window.contentView!, identifier: "task-monitor-button") as? NSButton else { throw RenderError.failed("Loading image is missing its monitor") }
    button.performClick(nil)
    try await Task.sleep(for: .milliseconds(200))
    try RenderChecks.capture(window, name: "image-generation-loading-monitor")
    model.imageGenerations[0].kind = .edit
    model.imageGenerations[0].status = "running"
    model.imageGenerations[0].activity = "Editing an image in Studio…"
    guard model.monitorTasks.first?.title == "Edit image · Studio" else { throw RenderError.failed("Editing row title is incorrect") }
    try await Task.sleep(for: .milliseconds(200))
    try RenderChecks.capture(window, name: "image-edit-running-monitor")
    model.closeChat()
}
