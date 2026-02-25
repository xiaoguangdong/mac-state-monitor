#!/usr/bin/swift

import AppKit
import Foundation

struct AssetEntry: Decodable {
    let assetType: String?
    let name: String?

    enum CodingKeys: String, CodingKey {
        case assetType = "AssetType"
        case name = "Name"
    }
}

func usage() -> Never {
    fputs("Usage: export_runcat_frames.swift <RunCatUIBundlePath> <OutputDir> [--white]\n", stderr)
    exit(2)
}

func runAssetutil(assetsCarPath: String) throws -> Data {
    let process = Process()
    process.executableURL = URL(fileURLWithPath: "/usr/bin/assetutil")
    process.arguments = ["-I", assetsCarPath]

    let stdoutPipe = Pipe()
    let stderrPipe = Pipe()
    process.standardOutput = stdoutPipe
    process.standardError = stderrPipe

    try process.run()
    // Drain stdout before waiting so large `assetutil` output does not block the process.
    let stdoutData = stdoutPipe.fileHandleForReading.readDataToEndOfFile()
    let stderrData = stderrPipe.fileHandleForReading.readDataToEndOfFile()

    process.waitUntilExit()
    if process.terminationStatus != 0 {
        let err = String(data: stderrData, encoding: .utf8) ?? ""
        throw NSError(domain: "export", code: Int(process.terminationStatus), userInfo: [
            NSLocalizedDescriptionKey: "assetutil failed: \(err)"
        ])
    }

    return stdoutData
}

func extractPrefixesAndFrames(from entries: [AssetEntry]) -> [String: [Int]] {
    var map: [String: [Int]] = [:]
    let regex = try? NSRegularExpression(pattern: #"^(.+)-page-(\d+)$"#, options: [])

    for entry in entries {
        guard entry.assetType == "Image", let name = entry.name else { continue }
        guard let regex else { continue }
        let nsName = name as NSString
        let range = NSRange(location: 0, length: nsName.length)
        guard let match = regex.firstMatch(in: name, options: [], range: range), match.numberOfRanges == 3 else {
            continue
        }
        let prefix = nsName.substring(with: match.range(at: 1))
        let indexStr = nsName.substring(with: match.range(at: 2))
        guard let index = Int(indexStr) else { continue }
        map[prefix, default: []].append(index)
    }

    for key in map.keys {
        map[key] = Array(Set(map[key] ?? [])).sorted()
    }
    return map
}

func pngData(from image: NSImage, forceWhite: Bool) -> Data? {
    guard let tiff = image.tiffRepresentation,
          let rep = NSBitmapImageRep(data: tiff) else {
        return nil
    }
    if forceWhite {
        guard let whiteRep = NSBitmapImageRep(
            bitmapDataPlanes: nil,
            pixelsWide: rep.pixelsWide,
            pixelsHigh: rep.pixelsHigh,
            bitsPerSample: 8,
            samplesPerPixel: 4,
            hasAlpha: true,
            isPlanar: false,
            colorSpaceName: .deviceRGB,
            bytesPerRow: 0,
            bitsPerPixel: 0
        ) else {
            return nil
        }
        let size = NSSize(width: rep.pixelsWide, height: rep.pixelsHigh)
        let rect = NSRect(origin: .zero, size: size)
        NSGraphicsContext.saveGraphicsState()
        if let ctx = NSGraphicsContext(bitmapImageRep: whiteRep) {
            NSGraphicsContext.current = ctx
            let cg = ctx.cgContext
            cg.clear(rect)
            image.draw(in: rect, from: .zero, operation: .copy, fraction: 1.0)
            cg.setBlendMode(.sourceIn)
            cg.setFillColor(NSColor.white.cgColor)
            cg.fill(rect)
            ctx.flushGraphics()
        }
        NSGraphicsContext.restoreGraphicsState()
        return whiteRep.representation(using: .png, properties: [:])
    }
    return rep.representation(using: .png, properties: [:])
}

if CommandLine.arguments.count != 3 && CommandLine.arguments.count != 4 {
    usage()
}

let bundlePath = CommandLine.arguments[1]
let outputDir = CommandLine.arguments[2]
let forceWhite = CommandLine.arguments.count == 4 && CommandLine.arguments[3] == "--white"
if CommandLine.arguments.count == 4 && !forceWhite {
    usage()
}
let assetsCarPath = "\(bundlePath)/Contents/Resources/Assets.car"

let fileManager = FileManager.default

do {
    guard fileManager.fileExists(atPath: bundlePath) else {
        throw NSError(domain: "export", code: 1, userInfo: [NSLocalizedDescriptionKey: "Bundle not found: \(bundlePath)"])
    }
    guard fileManager.fileExists(atPath: assetsCarPath) else {
        throw NSError(domain: "export", code: 1, userInfo: [NSLocalizedDescriptionKey: "Assets.car not found: \(assetsCarPath)"])
    }
    guard let bundle = Bundle(path: bundlePath) else {
        throw NSError(domain: "export", code: 1, userInfo: [NSLocalizedDescriptionKey: "Failed to load bundle: \(bundlePath)"])
    }

    if fileManager.fileExists(atPath: outputDir) {
        try fileManager.removeItem(atPath: outputDir)
    }
    try fileManager.createDirectory(atPath: outputDir, withIntermediateDirectories: true)

    let data = try runAssetutil(assetsCarPath: assetsCarPath)
    let entries = try JSONDecoder().decode([AssetEntry].self, from: data)
    let map = extractPrefixesAndFrames(from: entries)

    var exportedCount = 0
    let sortedPrefixes = map.keys.sorted()
    for prefix in sortedPrefixes {
        guard let indexes = map[prefix], !indexes.isEmpty else { continue }
        let prefixDir = "\(outputDir)/\(prefix)"
        try fileManager.createDirectory(atPath: prefixDir, withIntermediateDirectories: true)

        for idx in indexes {
            let name = "\(prefix)-page-\(idx)"
            guard let image = bundle.image(forResource: NSImage.Name(name)),
                  let png = pngData(from: image, forceWhite: forceWhite) else {
                continue
            }
            let outputPath = "\(prefixDir)/\(String(format: "%03d", idx)).png"
            try png.write(to: URL(fileURLWithPath: outputPath))
            exportedCount += 1
        }
    }

    if exportedCount == 0 {
        throw NSError(domain: "export", code: 1, userInfo: [NSLocalizedDescriptionKey: "No frames exported"])
    }
    print("Exported \(exportedCount) runner frames to \(outputDir)")
    exit(0)
} catch {
    fputs("Export failed: \(error.localizedDescription)\n", stderr)
    exit(1)
}
