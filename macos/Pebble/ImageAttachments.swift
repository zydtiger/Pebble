import AppKit
import SwiftUI

struct ImageAttachments: View {
    let images: [ImageAssetRecord]
    var remove: ((String) -> Void)?
    @State private var enlarged: ImageAssetRecord?

    var body: some View {
        ScrollView(.horizontal) {
            HStack(spacing: 8) {
                ForEach(images, id: \.id) { image in
                    ZStack(alignment: .topTrailing) {
                        Button { enlarged = image } label: {
                            if let preview = NSImage(contentsOfFile: image.thumbnailPath), FileManager.default.fileExists(atPath: image.sourcePath) {
                                Image(nsImage: preview).resizable().scaledToFill()
                                    .frame(width: remove == nil ? 180 : 72, height: remove == nil ? 140 : 72).clipped()
                            } else {
                                Label("Image unavailable", systemImage: "exclamationmark.triangle")
                                    .font(.system(size: 11)).frame(width: 180, height: 72)
                            }
                        }.buttonStyle(.plain).clipShape(RoundedRectangle(cornerRadius: 9))
                            .accessibilityLabel("Open image attachment").accessibilityIdentifier("image-\(image.id)")
                        if let remove {
                            Button { remove(image.id) } label: {
                                Image(systemName: "xmark.circle.fill").symbolRenderingMode(.palette)
                                    .foregroundStyle(.white, .black.opacity(0.6)).padding(3)
                            }.buttonStyle(.plain).accessibilityLabel("Remove image attachment")
                        }
                    }
                }
            }
        }.scrollIndicators(.hidden)
            .frame(maxWidth: min(560, CGFloat(images.count) * (remove == nil ? 180 : 72) + CGFloat(max(0, images.count - 1)) * 8))
            .fixedSize(horizontal: false, vertical: true)
        .sheet(isPresented: Binding(get: { enlarged != nil }, set: { if !$0 { enlarged = nil } })) {
            VStack(spacing: 12) {
                if let image = enlarged, let original = NSImage(contentsOfFile: image.sourcePath) {
                    Image(nsImage: original).resizable().scaledToFit().frame(maxWidth: 1000, maxHeight: 760)
                } else { Text("Image attachment is missing or corrupt.") }
                Button("Done") { enlarged = nil }.buttonStyle(SoftButtonStyle()).keyboardShortcut(.cancelAction)
            }.padding(20).frame(minWidth: 420, minHeight: 300).background(PebbleTheme.canvas).foregroundStyle(PebbleTheme.ink)
        }
    }
}
