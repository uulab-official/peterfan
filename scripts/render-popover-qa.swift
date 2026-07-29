#!/usr/bin/env swift
import AppKit
import Foundation

let outputPath = CommandLine.arguments.dropFirst().first ?? "docs/images/peterfan-popover-qa.png"
let version = readWorkspaceVersion() ?? "dev"
let scale: CGFloat = 2
let cellSize = NSSize(width: 360, height: 520)
let gutter: CGFloat = 28
let margin: CGFloat = 34
let size = NSSize(
    width: margin * 2 + cellSize.width * 2 + gutter,
    height: margin * 2 + cellSize.height * 2 + gutter
)
let image = NSImage(size: size)

struct Palette {
    let background: NSColor
    let panel: NSColor
    let section: NSColor
    let line: NSColor
    let text: NSColor
    let dim: NSColor
    let accent: NSColor
    let green: NSColor
    let yellow: NSColor
    let red: NSColor
}

struct CaseSpec {
    let title: String
    let language: String
    let palette: Palette
    let isKorean: Bool
    let isDark: Bool
}

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

let dark = Palette(
    background: c(7, 8, 10),
    panel: c(27, 28, 32),
    section: c(255, 255, 255, 0.035),
    line: c(255, 255, 255, 0.10),
    text: c(242, 244, 248),
    dim: c(142, 151, 166),
    accent: c(91, 157, 255),
    green: c(48, 209, 88),
    yellow: c(255, 204, 0),
    red: c(255, 69, 58)
)

let light = Palette(
    background: c(236, 238, 242),
    panel: c(250, 251, 253),
    section: c(255, 255, 255, 0.72),
    line: c(42, 47, 56, 0.14),
    text: c(29, 32, 37),
    dim: c(105, 116, 132),
    accent: c(28, 92, 184),
    green: c(19, 150, 67),
    yellow: c(184, 136, 0),
    red: c(210, 45, 38)
)

func rounded(_ rect: NSRect, radius: CGFloat, fill: NSColor, stroke: NSColor? = nil, width: CGFloat = 1) {
    let path = NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius)
    fill.setFill()
    path.fill()
    if let stroke {
        stroke.setStroke()
        path.lineWidth = width
        path.stroke()
    }
}

func text(_ value: String, _ rect: NSRect, _ size: CGFloat, _ color: NSColor, weight: NSFont.Weight = .regular, align: NSTextAlignment = .left) {
    let paragraph = NSMutableParagraphStyle()
    paragraph.alignment = align
    paragraph.lineBreakMode = .byTruncatingTail
    let attrs: [NSAttributedString.Key: Any] = [
        .font: NSFont.systemFont(ofSize: size, weight: weight),
        .foregroundColor: color,
        .paragraphStyle: paragraph,
    ]
    NSString(string: value).draw(in: rect, withAttributes: attrs)
}

func meter(_ rect: NSRect, value: CGFloat, color: NSColor, palette: Palette) {
    rounded(rect, radius: rect.height / 2, fill: palette.line)
    rounded(NSRect(x: rect.minX, y: rect.minY, width: max(6, rect.width * value), height: rect.height), radius: rect.height / 2, fill: color)
}

func sparkline(_ rect: NSRect, values: [CGFloat], palette: Palette) {
    let baseline = NSBezierPath()
    baseline.move(to: CGPoint(x: rect.minX, y: rect.minY + 8))
    baseline.line(to: CGPoint(x: rect.maxX, y: rect.minY + 8))
    palette.line.setStroke()
    baseline.lineWidth = 1
    baseline.stroke()

    let path = NSBezierPath()
    for (index, value) in values.enumerated() {
        let x = rect.minX + CGFloat(index) * rect.width / CGFloat(max(1, values.count - 1))
        let y = rect.minY + 10 + value * (rect.height - 16)
        if index == 0 {
            path.move(to: CGPoint(x: x, y: y))
        } else {
            path.line(to: CGPoint(x: x, y: y))
        }
    }
    palette.accent.setStroke()
    path.lineWidth = 2
    path.lineJoinStyle = .round
    path.lineCapStyle = .round
    path.stroke()
}

func label(_ english: String, _ korean: String, _ spec: CaseSpec) -> String {
    spec.isKorean ? korean : english
}

func drawCase(_ spec: CaseSpec, origin: CGPoint) {
    let p = spec.palette
    rounded(NSRect(origin: origin, size: cellSize), radius: 16, fill: p.background, stroke: p.line)
    text(spec.title, NSRect(x: origin.x + 18, y: origin.y + cellSize.height - 32, width: 160, height: 18), 11, p.dim, weight: .semibold)
    text("v\(version)", NSRect(x: origin.x + cellSize.width - 92, y: origin.y + cellSize.height - 32, width: 72, height: 18), 11, p.accent, weight: .bold, align: .right)

    let panel = NSRect(x: origin.x + 18, y: origin.y + 18, width: 262, height: 474)
    let rail = NSRect(x: origin.x + 288, y: origin.y + 18, width: 54, height: 474)
    rounded(panel, radius: 12, fill: p.panel, stroke: p.line)
    rounded(rail, radius: 12, fill: p.panel, stroke: p.line)

    text("PeterFan", NSRect(x: panel.minX + 18, y: panel.maxY - 42, width: 110, height: 18), 14, p.text, weight: .bold)
    let ranges = ["2m", "1h", "1d"]
    for (index, range) in ranges.enumerated() {
        let x = panel.maxX - 90 + CGFloat(index) * 25
        let selected = index == 0
        rounded(
            NSRect(x: x, y: panel.maxY - 42, width: 22, height: 17),
            radius: 8.5,
            fill: selected ? p.accent.withAlphaComponent(spec.isDark ? 0.24 : 0.15) : p.section
        )
        text(range, NSRect(x: x, y: panel.maxY - 39, width: 22, height: 10), 7.2, selected ? p.accent : p.dim, weight: .bold, align: .center)
    }

    let summary: [(String, String, CGFloat, NSColor)] = [
        (label("CPU", "CPU", spec), "57%", 0.57, p.green),
        (label("Memory", "메모리", spec), "73%", 0.73, p.accent),
        (label("CPU temperature", "CPU 온도", spec), "61°C", 0.61, p.yellow),
        (label("Fan average", "팬 평균 RPM", spec), "3865", 0.52, p.accent),
    ]
    let summaryGap: CGFloat = 7
    let summaryWidth = (226 - summaryGap) / 2
    let summaryHeight: CGFloat = 56
    for (index, item) in summary.enumerated() {
        let column = index % 2
        let row = index / 2
        let x = panel.minX + 18 + CGFloat(column) * (summaryWidth + summaryGap)
        let y = panel.maxY - 111 - CGFloat(row) * (summaryHeight + summaryGap)
        rounded(NSRect(x: x, y: y, width: summaryWidth, height: summaryHeight), radius: 7, fill: p.section, stroke: p.line)
        text(item.0, NSRect(x: x + 10, y: y + 35, width: summaryWidth - 20, height: 12), 8.2, p.dim, weight: .semibold)
        text(item.1, NSRect(x: x + 10, y: y + 16, width: summaryWidth - 20, height: 19), 15, item.3, weight: .bold)
        meter(NSRect(x: x + 10, y: y + 9, width: summaryWidth - 20, height: 3), value: item.2, color: item.3, palette: p)
    }

    text(label("CPU activity", "CPU 사용량", spec), NSRect(x: panel.minX + 18, y: panel.maxY - 204, width: 120, height: 14), 9.5, p.dim, weight: .semibold)
    let barsY = panel.maxY - 229
    for i in 0..<16 {
        let color = i < 4 ? p.red : (i < 12 ? p.green : p.yellow)
        let height: CGFloat = 8 + CGFloat((i * 7) % 16)
        rounded(NSRect(x: panel.minX + 18 + CGFloat(i) * 14, y: barsY, width: 10, height: height), radius: 2, fill: color)
    }
    sparkline(NSRect(x: panel.minX + 18, y: panel.maxY - 266, width: 226, height: 34), values: [0.2, 0.18, 0.22, 0.19, 0.28, 0.26, 0.34, 0.33, 0.50, 0.54, 0.51, 0.56], palette: p)

    let rows: [(String, String, CGFloat, NSColor)] = [
        (label("Memory", "메모리", spec), "73.3%", 0.73, p.yellow),
        (label("CPU avg temp", "CPU 평균 온도", spec), "61°C", 0.61, p.yellow),
    ]
    for (index, row) in rows.enumerated() {
        let y = panel.maxY - 328 - CGFloat(index) * 62
        text(row.0, NSRect(x: panel.minX + 18, y: y + 28, width: 128, height: 17), 10.5, p.dim, weight: .semibold)
        text(row.1, NSRect(x: panel.maxX - 92, y: y + 28, width: 72, height: 17), 12, p.text, weight: .bold, align: .right)
        meter(NSRect(x: panel.minX + 18, y: y + 13, width: 226, height: 5), value: row.2, color: row.3, palette: p)
    }

    text(label("All sensors · 18", "전체 센서 · 18", spec), NSRect(x: panel.minX + 18, y: panel.minY + 24, width: 226, height: 16), 9.5, p.dim, weight: .semibold)

    let railLabels = [
        label("Status", "상태", spec),
        label("Fans", "팬 제어", spec),
        label("Settings", "설정", spec),
        label("System", "시스템", spec),
    ]
    let railItemHeight: CGFloat = 58
    let railGap: CGFloat = 10
    let railBottomInset: CGFloat = 18
    for (index, item) in railLabels.enumerated() {
        let y = rail.minY + railBottomInset + CGFloat(railLabels.count - 1 - index) * (railItemHeight + railGap)
        let fill = index == 0 ? p.accent.withAlphaComponent(spec.isDark ? 0.24 : 0.15) : p.section
        rounded(NSRect(x: rail.minX + 6, y: y, width: 42, height: railItemHeight), radius: 8, fill: fill, stroke: p.line)
        text(item, NSRect(x: rail.minX + 8, y: y + 11, width: 38, height: 15), 7.4, index == 0 ? p.accent : p.text, weight: .bold, align: .center)
    }
}

image.lockFocus()
c(18, 20, 24).setFill()
NSRect(origin: .zero, size: size).fill()
text("PeterFan Popover Visual QA", NSRect(x: margin, y: size.height - 30, width: 320, height: 18), 14, c(234, 238, 246), weight: .bold)
text("dark/light · English/Korean · current four-item rail check", NSRect(x: margin + 230, y: size.height - 30, width: 420, height: 18), 11, c(151, 161, 176))

let cases = [
    CaseSpec(title: "Dark / English", language: "en", palette: dark, isKorean: false, isDark: true),
    CaseSpec(title: "Dark / Korean", language: "ko", palette: dark, isKorean: true, isDark: true),
    CaseSpec(title: "Light / English", language: "en", palette: light, isKorean: false, isDark: false),
    CaseSpec(title: "Light / Korean", language: "ko", palette: light, isKorean: true, isDark: false),
]

for (index, spec) in cases.enumerated() {
    let column = index % 2
    let row = index / 2
    drawCase(
        spec,
        origin: CGPoint(
            x: margin + CGFloat(column) * (cellSize.width + gutter),
            y: margin + CGFloat(1 - row) * (cellSize.height + gutter)
        )
    )
}

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
