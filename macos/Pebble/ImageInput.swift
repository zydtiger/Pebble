import AppKit
import ImageIO
import UniformTypeIdentifiers

enum ImageImportSource: Sendable {
    case file(URL)
    case data(Data)
}

enum ImageInput {
    static let byteLimit = 12 * 1024 * 1024
    static let pasteTypes: [NSPasteboard.PasteboardType] = [.fileURL, .png, .tiff, .init("public.jpeg")]

    @MainActor
    static func read(_ pasteboard: NSPasteboard) throws -> [ImageImportSource]? {
        if let urls = pasteboard.readObjects(forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL], !urls.isEmpty {
            return urls.map(ImageImportSource.file)
        }
        for type in [NSPasteboard.PasteboardType.png, .init("public.jpeg")] {
            if let data = pasteboard.data(forType: type) {
                guard data.count <= byteLimit else { throw InputError.tooLarge }
                return [.data(data)]
            }
        }
        // macOS screenshots and copied images commonly use TIFF. Convert only this
        // platform pasteboard input; file imports remain static JPEG/PNG in Rust.
        if let data = pasteboard.data(forType: .tiff) {
            guard data.count <= byteLimit,
                  let source = CGImageSourceCreateWithData(data as CFData, nil),
                  CGImageSourceGetCount(source) == 1,
                  let properties = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
                  let width = properties[kCGImagePropertyPixelWidth] as? Int,
                  let height = properties[kCGImagePropertyPixelHeight] as? Int,
                  width > 0, height > 0, width <= 20_000, height <= 20_000,
                  width * height <= 20_000_000,
                  let image = CGImageSourceCreateThumbnailAtIndex(source, 0, [kCGImageSourceCreateThumbnailFromImageAlways: true, kCGImageSourceCreateThumbnailWithTransform: true, kCGImageSourceThumbnailMaxPixelSize: max(width, height)] as CFDictionary) else { throw InputError.tooLarge }
            let result = NSMutableData()
            guard let destination = CGImageDestinationCreateWithData(result, UTType.png.identifier as CFString, 1, nil) else { throw InputError.conversion }
            CGImageDestinationAddImage(destination, image, nil)
            guard CGImageDestinationFinalize(destination), result.length <= byteLimit else { throw InputError.tooLarge }
            return [.data(result as Data)]
        }
        return nil
    }
    enum InputError: LocalizedError {
        case tooLarge, conversion
        var errorDescription: String? {
            switch self {
            case .tooLarge: "Image attachment exceeds the 12 MiB or 20 million pixel limit."
            case .conversion: "Could not convert the clipboard image to PNG."
            }
        }
    }
}
