// Writes the pictures the OCR tests read: sample.png (one picture of two lines of text) and scanned.pdf
// (two pages that are only pictures, with no text layer). Run: swift make.swift <output folder>
import AppKit
import PDFKit

func picture(_ lines: [String]) -> NSImage {
    let size = NSSize(width: 1000, height: 120 + 90 * CGFloat(lines.count))
    let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil, pixelsWide: Int(size.width), pixelsHigh: Int(size.height), bitsPerSample: 8,
        samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0,
        bitsPerPixel: 0)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSColor.white.setFill()
    NSRect(origin: .zero, size: size).fill()
    let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont(name: "Helvetica", size: 48)!, .foregroundColor: NSColor.black,
    ]
    for (i, line) in lines.enumerated() {
        NSString(string: line).draw(at: NSPoint(x: 40, y: size.height - 110 - 90 * CGFloat(i)), withAttributes: attrs)
    }
    NSGraphicsContext.restoreGraphicsState()
    let image = NSImage(size: size)
    image.addRepresentation(rep)
    return image
}

let out = URL(fileURLWithPath: CommandLine.arguments[1])
let sample = picture(["Invoice 4821", "Total due 90 dollars by June 30"])
let rep = NSBitmapImageRep(data: sample.tiffRepresentation!)!
try! rep.representation(using: .png, properties: [:])!.write(to: out.appendingPathComponent("sample.png"))

let doc = PDFDocument()
doc.insert(PDFPage(image: picture(["Candidate instructions", "Write 1000 words at most"]))!, at: 0)
doc.insert(PDFPage(image: picture(["Second page", "Submit by Friday"]))!, at: 1)
doc.write(to: out.appendingPathComponent("scanned.pdf"))
