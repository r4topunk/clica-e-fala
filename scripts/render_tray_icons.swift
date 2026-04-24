#!/usr/bin/swift

// Renders tray icons as colored rounded chips with a white SF Symbol glyph on top.
// Run from project root:
//     swift scripts/render_tray_icons.swift
// Requires macOS 11+.

import AppKit
import Foundation

struct Icon {
    let file: String
    let symbol: String
    let color: NSColor
}

let icons: [Icon] = [
    Icon(file: "tray_idle",         symbol: "mic.fill",           color: NSColor(calibratedWhite: 0.35, alpha: 1.0)),
    Icon(file: "tray_recording",    symbol: "record.circle.fill", color: .systemRed),
    Icon(file: "tray_transcribing", symbol: "waveform",           color: .systemTeal),
    Icon(file: "tray_refining",     symbol: "wand.and.stars",     color: .systemGreen),
    Icon(file: "tray_review",       symbol: "pencil.tip",         color: .systemPink),
]

let outDir = "src-tauri/icons"
let canvasPt: CGFloat = 22   // menubar-ish size in points
let cornerPt: CGFloat = 6
let insetPt:  CGFloat = 4    // breathing room around glyph
let scale:    CGFloat = 2.0

for icon in icons {
    let pxW = Int(canvasPt * scale)
    let pxH = Int(canvasPt * scale)

    guard let rep = NSBitmapImageRep(
        bitmapDataPlanes: nil,
        pixelsWide: pxW, pixelsHigh: pxH,
        bitsPerSample: 8, samplesPerPixel: 4,
        hasAlpha: true, isPlanar: false,
        colorSpaceName: .deviceRGB,
        bytesPerRow: 0, bitsPerPixel: 0
    ) else { continue }
    rep.size = NSSize(width: canvasPt, height: canvasPt)

    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
    NSGraphicsContext.current?.imageInterpolation = .high

    // Rounded rect background in state color.
    let bgRect = NSRect(x: 0, y: 0, width: canvasPt, height: canvasPt)
    let bgPath = NSBezierPath(roundedRect: bgRect, xRadius: cornerPt, yRadius: cornerPt)
    icon.color.setFill()
    bgPath.fill()

    // Symbol on top, white, scaled to fit inset box, centered.
    let cfg = NSImage.SymbolConfiguration(paletteColors: [.white])
    if let raw = NSImage(systemSymbolName: icon.symbol, accessibilityDescription: nil)?
                    .withSymbolConfiguration(cfg) {
        let maxSide = canvasPt - 2 * insetPt
        let intrinsic = raw.size
        let factor = min(maxSide / intrinsic.width, maxSide / intrinsic.height)
        let drawSize = NSSize(width: intrinsic.width * factor,
                              height: intrinsic.height * factor)
        let origin = NSPoint(
            x: (canvasPt - drawSize.width)  / 2,
            y: (canvasPt - drawSize.height) / 2
        )
        let rect = NSRect(origin: origin, size: drawSize)
        raw.draw(in: rect, from: .zero, operation: .sourceOver, fraction: 1.0)
    } else {
        print("✗ symbol not found: \(icon.symbol)")
    }

    NSGraphicsContext.restoreGraphicsState()

    guard let data = rep.representation(using: .png, properties: [:]) else { continue }
    let outURL = URL(fileURLWithPath: "\(outDir)/\(icon.file).png")
    do {
        try data.write(to: outURL)
        print("✓ \(outURL.path) (\(pxW)x\(pxH))")
    } catch {
        print("✗ write failed \(outURL.path): \(error)")
    }
}
