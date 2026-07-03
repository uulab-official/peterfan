#!/usr/bin/env swift
import AppKit
import Foundation

let outputPath = CommandLine.arguments.dropFirst().first ?? "docs/images/peterfan-readme-overview.png"
let version = readWorkspaceVersion() ?? "dev"
let size = NSSize(width: 1600, height: 900)
let image = NSImage(size: size)

func readWorkspaceVersion() -> String? {
    guard let cargo = try? String(contentsOfFile: "Cargo.toml", encoding: .utf8) else { return nil }
    var inWorkspacePackage = false
    for rawLine in cargo.split(separator: "\n") {
        let line = rawLine.trimmingCharacters(in: .whitespaces)
        if line == "[workspace.package]" {
            inWorkspacePackage = true
            continue
        }
        if line.hasPrefix("[") && line != "[workspace.package]" {
            inWorkspacePackage = false
        }
        if inWorkspacePackage && line.hasPrefix("version = ") {
            return line.split(separator: "\"").dropFirst().first.map(String.init)
        }
    }
    return nil
}

func c(_ r: CGFloat, _ g: CGFloat, _ b: CGFloat, _ a: CGFloat = 1) -> NSColor {
    NSColor(calibratedRed: r / 255, green: g / 255, blue: b / 255, alpha: a)
}

func rounded(_ rect: NSRect, radius: CGFloat, fill: NSColor, stroke: NSColor? = nil) {
    let path = NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)
    fill.setFill()
    path.fill()
    if let stroke {
        stroke.setStroke()
        path.lineWidth = 1.5
        path.stroke()
    }
}

func text(_ value: String, _ x: CGFloat, _ y: CGFloat, _ size: CGFloat, _ color: NSColor, weight: NSFont.Weight = .regular) {
    let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: size, weight: weight),
        .foregroundColor: color,
    ]
    NSString(string: value).draw(at: NSPoint(x: x, y: y), withAttributes: attrs)
}

func line(_ points: [CGPoint], color: NSColor, width: CGFloat) {
    guard let first = points.first else { return }
    let path = NSBezierPath()
    path.move(to: first)
    for point in points.dropFirst() {
        path.line(to: point)
    }
    color.setStroke()
    path.lineWidth = width
    path.lineJoinStyle = .round
    path.lineCapStyle = .round
    path.stroke()
}

func meter(_ x: CGFloat, _ y: CGFloat, _ width: CGFloat, _ value: CGFloat, _ color: NSColor) {
    rounded(NSRect(x: x, y: y, width: width, height: 8), radius: 4, fill: c(255, 255, 255, 0.10))
    rounded(NSRect(x: x, y: y, width: max(10, width * value), height: 8), radius: 4, fill: color)
}

image.lockFocus()
c(8, 10, 14).setFill()
NSRect(origin: .zero, size: size).fill()

let wave = NSBezierPath()
wave.move(to: CGPoint(x: 0, y: 720))
wave.curve(to: CGPoint(x: 1600, y: 780), controlPoint1: CGPoint(x: 430, y: 620), controlPoint2: CGPoint(x: 1040, y: 905))
wave.line(to: CGPoint(x: 1600, y: 900))
wave.line(to: CGPoint(x: 0, y: 900))
wave.close()
c(47, 96, 170, 0.28).setFill()
wave.fill()

rounded(NSRect(x: 118, y: 84, width: 1364, height: 732), radius: 34, fill: c(17, 18, 22), stroke: c(255, 255, 255, 0.10))
rounded(NSRect(x: 118, y: 756, width: 1364, height: 60), radius: 34, fill: c(32, 34, 40), stroke: c(255, 255, 255, 0.08))

text("PeterFan", 164, 775, 24, c(245, 247, 250), weight: .bold)
text("Menu-bar monitor - fan control - CLI diagnostics", 296, 780, 17, c(159, 169, 184))
let spark: [CGFloat] = [0.28, 0.36, 0.31, 0.54, 0.48, 0.62, 0.43, 0.55, 0.71, 0.68, 0.52, 0.44, 0.49, 0.59, 0.76, 0.73, 0.61, 0.66]
for (i, value) in spark.enumerated() {
    rounded(NSRect(x: 1250 + CGFloat(i * 10), y: 773, width: 7, height: 26 * value), radius: 3, fill: c(48, 209, 88, 0.9))
}
text("54 C", 1430, 774, 18, c(245, 247, 250), weight: .semibold)

let panel = NSRect(x: 164, y: 142, width: 622, height: 560)
rounded(panel, radius: 18, fill: c(27, 28, 32), stroke: c(255, 255, 255, 0.10))
text("Ready", 202, 658, 22, c(245, 247, 250), weight: .bold)
text("app v\(version) - daemon v1.26.24 compatible - login on", 202, 632, 15, c(143, 153, 169))
rounded(NSRect(x: 202, y: 586, width: 130, height: 34), radius: 9, fill: c(64, 116, 216))
text("App", 252, 594, 15, .white, weight: .semibold)
rounded(NSRect(x: 346, y: 586, width: 130, height: 34), radius: 9, fill: c(255, 255, 255, 0.08))
text("Login On", 372, 594, 15, c(231, 235, 242), weight: .semibold)

let cards: [(String, String, CGFloat, NSColor)] = [
    ("CPU", "38%", 0.38, c(91, 157, 255)),
    ("Memory", "64%", 0.64, c(175, 112, 255)),
    ("Temp", "54 C", 0.54, c(48, 209, 88)),
    ("Network", "1.4 MB/s", 0.28, c(255, 204, 68)),
]
for (index, card) in cards.enumerated() {
    let x = 202 + CGFloat(index % 2) * 278
    let y = 454 - CGFloat(index / 2) * 104
    rounded(NSRect(x: x, y: y, width: 246, height: 78), radius: 13, fill: c(255, 255, 255, 0.045), stroke: c(255, 255, 255, 0.07))
    text(card.0, x + 18, y + 48, 14, c(151, 161, 176))
    text(card.1, x + 18, y + 20, 28, c(246, 248, 252), weight: .bold)
    meter(x + 122, y + 28, 96, card.2, card.3)
}

let chart = NSRect(x: 202, y: 280, width: 520, height: 116)
rounded(chart, radius: 13, fill: c(255, 255, 255, 0.045), stroke: c(255, 255, 255, 0.07))
text("Temperature - CPU avg", 220, 364, 15, c(205, 212, 224), weight: .semibold)
let temps: [CGFloat] = [0.24, 0.29, 0.34, 0.30, 0.41, 0.38, 0.48, 0.45, 0.56, 0.51, 0.44, 0.49, 0.58, 0.63, 0.60, 0.66, 0.62, 0.69]
let points = temps.enumerated().map { index, value in
    CGPoint(x: 226 + CGFloat(index) * 27.5, y: 302 + value * 54)
}
line(points, color: c(48, 209, 88), width: 4)

text("Fans", 202, 242, 17, c(245, 247, 250), weight: .bold)
for row in 0..<2 {
    let y = 212 - CGFloat(row * 34)
    text(row == 0 ? "Left fan" : "Right fan", 202, y, 14, c(191, 199, 211))
    text(row == 0 ? "2440 RPM" : "2388 RPM", 316, y, 14, c(239, 242, 247), weight: .semibold)
    meter(418, y + 4, 210, row == 0 ? 0.43 : 0.41, c(91, 157, 255))
}

let term = NSRect(x: 828, y: 142, width: 590, height: 560)
rounded(term, radius: 18, fill: c(12, 13, 16), stroke: c(255, 255, 255, 0.10))
rounded(NSRect(x: 828, y: 664, width: 590, height: 38), radius: 18, fill: c(35, 37, 43), stroke: c(255, 255, 255, 0.06))
text("peterfan doctor", 858, 673, 16, c(220, 226, 238), weight: .semibold)
let terminalLines: [(String, NSColor, NSFont.Weight)] = [
    ("PeterFan doctor", c(91, 205, 255), .bold),
    ("Version:          \(version)", c(224, 231, 242), .medium),
    ("OS / arch:        macos / aarch64", c(224, 231, 242), .medium),
    ("Metrics backend:  sysinfo", c(224, 231, 242), .medium),
    ("Thermal backend:  macos", c(224, 231, 242), .medium),
    ("", .white, .regular),
    ("ok cpu   ok memory   ok battery", c(48, 209, 88), .medium),
    ("ok read temperatures   ok read fans", c(48, 209, 88), .medium),
    ("ok peterfand daemon reachable", c(48, 209, 88), .medium),
    ("", .white, .regular),
    ("Setup", c(91, 205, 255), .bold),
    ("app version:          v\(version)", c(224, 231, 242), .medium),
    ("daemon requirement:   >= v1.26.22", c(224, 231, 242), .medium),
    ("installed daemon:     v1.26.24 compatible", c(48, 209, 88), .medium),
    ("daemon_update_required: false", c(143, 153, 169), .medium),
]
var y: CGFloat = 626
for lineText in terminalLines {
    if lineText.0.isEmpty {
        y -= 17
        continue
    }
    text(lineText.0, 858, y, 17, lineText.1, weight: lineText.2)
    y -= 30
}

text("Open source core - signed DMG releases - local-first diagnostics", 164, 48, 20, c(167, 178, 194), weight: .medium)
image.unlockFocus()

guard let tiff = image.tiffRepresentation,
      let rep = NSBitmapImageRep(data: tiff),
      let data = rep.representation(using: .png, properties: [:]) else {
    fputs("error: could not render PNG\n", stderr)
    exit(1)
}

let outputURL = URL(fileURLWithPath: outputPath)
try FileManager.default.createDirectory(at: outputURL.deletingLastPathComponent(), withIntermediateDirectories: true)
try data.write(to: outputURL)
print(outputPath)
