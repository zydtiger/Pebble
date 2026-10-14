import Foundation

@MainActor
func exerciseImageGenerationBridge(_ root: URL) async throws {
    let model = ChatModel(dataRoot: root)
    await model.load()
    try require(model.error == nil, "Image bridge workspace did not load")
    let settings = ImageGenerationSettingsRecord(
        studio: StudioSettingsRecord(baseUrl: "https://api.example.com", modelId: "synthetic-model", gpuId: "synthetic-gpu", steps: nil, guidance: nil),
        chatgpt: ChatGptImageSettingsRecord(model: nil, effort: nil))
    try await model.client.saveImageSettings(settings)
    let restored = try await model.client.imageSettings()
    try require(restored.studio?.modelId == "synthetic-model" && restored.studio?.steps == nil && restored.chatgpt != nil, "Typed image settings lost defaults")
    let room = model.selectedID ?? ""
    let persona = model.selected?.id ?? ""
    let queued = ImageGenerationRecord(id: "synthetic-image", personaId: persona, conversationId: room, backend: "studio", status: "queued", activity: "Waiting in Studio's queue…", detail: "", publicationStatus: "pending", assetId: nil, actualWidth: nil, actualHeight: nil)
    model.imageGenerations = [queued]
    try require(model.monitorTasks.count == 1, "Queued image is hidden after acknowledgement")
    var terminal = queued
    terminal.status = "completed"; terminal.publicationStatus = "queued"
    terminal.assetId = "immutable-image"; terminal.actualWidth = 1254; terminal.actualHeight = 1254
    model.imageGenerations = [terminal]
    try require(model.monitorTasks.isEmpty, "Terminal generation monitor or queued publication status is incorrect")
    terminal.publicationStatus = "completed"; model.imageGenerations = [terminal]
    try require(model.monitorTasks.isEmpty, "Completed image work remains active")
    model.closeChat()
}
