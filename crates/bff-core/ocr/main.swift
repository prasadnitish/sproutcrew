// Reads text out of an image or out of PDF pages with Apple's Vision framework. It runs as a child of
// BFF so a damaged file can only stop this process. One JSON object per line goes to stdout:
//   {"page":3,"text":"..."}   a page that was read (an image is page 1)
//   {"error":"..."}           the file could not be opened or read at all
//   {"warm":true}             the reader is ready (see `warm`)
// Usage: bff-ocr warm
//        bff-ocr image <file>
//        bff-ocr pdf <file> <page numbers, 1 based, separated by commas>
import AppKit
import Foundation
import PDFKit
import Vision

func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object),
        let line = String(data: data, encoding: .utf8)
    else { return }
    print(line)
    fflush(stdout)
}

func fail(_ why: String) -> Never {
    emit(["error": why])
    exit(1)
}

/// The text of one image, a line per row of text in reading order.
func recognise(_ handler: VNImageRequestHandler) throws -> String {
    let request = VNRecognizeTextRequest()
    request.recognitionLevel = .accurate
    request.usesLanguageCorrection = true
    request.automaticallyDetectsLanguage = true
    try handler.perform([request])
    let found = (request.results ?? []).compactMap { obs -> (String, CGRect)? in
        guard let best = obs.topCandidates(1).first else { return nil }
        return (best.string, obs.boundingBox)
    }
    // Vision's origin is the bottom left, so the top row has the largest y.
    let ordered = found.sorted { $0.1.midY > $1.1.midY }
    var rows: [[(String, CGRect)]] = []
    for item in ordered {
        if let last = rows.last?.first, abs(last.1.midY - item.1.midY) < 0.5 * min(last.1.height, item.1.height) {
            rows[rows.count - 1].append(item)
        } else {
            rows.append([item])
        }
    }
    return rows
        .map { $0.sorted { $0.1.minX < $1.1.minX }.map { $0.0 }.joined(separator: " ") }
        .joined(separator: "\n")
}

let args = CommandLine.arguments

// The first time a new copy of this program reads anything, macOS spends about half a minute getting its
// text model ready. `warm` pays that on a small made-up picture, so BFF can do it in the background.
if args.count == 2 && args[1] == "warm" {
    let side = 256
    guard let context = CGContext(
        data: nil, width: side * 2, height: side / 2, bitsPerComponent: 8, bytesPerRow: 0,
        space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue),
        let _ = Optional(context.setFillColor(CGColor(gray: 1, alpha: 1))),
        let _ = Optional(context.fill(CGRect(x: 0, y: 0, width: side * 2, height: side / 2)))
    else { fail("no drawing surface") }
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(cgContext: context, flipped: false)
    NSString(string: "Hello BFF").draw(
        at: NSPoint(x: 20, y: 40), withAttributes: [.font: NSFont.systemFont(ofSize: 60), .foregroundColor: NSColor.black])
    NSGraphicsContext.restoreGraphicsState()
    guard let image = context.makeImage() else { fail("no picture") }
    _ = try? recognise(VNImageRequestHandler(cgImage: image, options: [:]))
    emit(["warm": true])
    exit(0)
}

guard args.count >= 3 else { fail("usage: bff-ocr image <file> | bff-ocr pdf <file> <pages>") }
let url = URL(fileURLWithPath: args[2])

switch args[1] {
case "image":
    do {
        emit(["page": 1, "text": try recognise(VNImageRequestHandler(url: url, options: [:]))])
    } catch {
        fail("the image could not be read: \(error.localizedDescription)")
    }
case "pdf":
    guard args.count >= 4 else { fail("no pages were named") }
    guard let doc = PDFDocument(url: url) else { fail("the PDF could not be opened") }
    if doc.isLocked { fail("the PDF is locked with a password") }
    for number in args[3].split(separator: ",").compactMap({ Int($0) }) {
        autoreleasepool {
            guard number >= 1, number <= doc.pageCount, let page = doc.page(at: number - 1) else { return }
            // About 200 dots per inch on a letter page, and never more than 3000 dots on the long side.
            let box = page.bounds(for: .mediaBox)
            let longest = max(box.width, box.height, 1)
            let scale = min(max(2200 / longest, 1), 3000 / longest)
            let size = NSSize(width: box.width * scale, height: box.height * scale)
            let picture = page.thumbnail(of: size, for: .mediaBox)
            guard let image = picture.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
                emit(["page": number, "text": ""])
                return
            }
            do {
                emit(["page": number, "text": try recognise(VNImageRequestHandler(cgImage: image, options: [:]))])
            } catch {
                emit(["page": number, "text": ""])
            }
        }
    }
default:
    fail("unknown command")
}
