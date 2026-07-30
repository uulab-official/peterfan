#!/usr/bin/env swift
import AppKit
import Foundation

let outputPath = CommandLine.arguments.dropFirst().first ?? "docs/images/peterfan-popover-qa.png"
let version = readWorkspaceVersion() ?? "dev"
let cellSize = NSSize(width: 440, height: 520)
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
    let view: String
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
    background: c(12, 13, 15),
    panel: c(24, 26, 29),
    section: c(37, 40, 45),
    line: c(255, 255, 255, 0.085),
    text: c(243, 244, 246),
    dim: c(150, 157, 168),
    accent: c(110, 168, 255),
    green: c(93, 216, 121),
    yellow: c(244, 201, 93),
    red: c(255, 107, 99)
)

let light = Palette(
    background: c(238, 240, 243),
    panel: c(244, 245, 247),
    section: c(255, 255, 255),
    line: c(25, 31, 40, 0.10),
    text: c(32, 35, 41),
    dim: c(105, 113, 126),
    accent: c(37, 103, 189),
    green: c(35, 159, 82),
    yellow: c(180, 122, 0),
    red: c(216, 59, 53)
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

func line(from start: CGPoint, to end: CGPoint, color: NSColor, width: CGFloat = 1) {
    let path = NSBezierPath()
    path.move(to: start)
    path.line(to: end)
    color.setStroke()
    path.lineWidth = width
    path.stroke()
}

func drawRailIcon(_ index: Int, in rect: NSRect, color: NSColor) {
    color.setStroke()
    let path = NSBezierPath()
    path.lineWidth = 1.5
    path.lineCapStyle = .round
    path.lineJoinStyle = .round

    switch index {
    case 0:
        path.appendRoundedRect(NSRect(x: rect.minX + 3, y: rect.minY + 3, width: rect.width - 6, height: rect.height - 6), xRadius: 2, yRadius: 2)
        path.move(to: CGPoint(x: rect.minX + 7, y: rect.midY + 2))
        path.line(to: CGPoint(x: rect.maxX - 7, y: rect.midY + 2))
        path.move(to: CGPoint(x: rect.minX + 7, y: rect.midY - 3))
        path.line(to: CGPoint(x: rect.midX + 2, y: rect.midY - 3))
    case 1:
        path.appendOval(in: NSRect(x: rect.midX - 2, y: rect.midY - 2, width: 4, height: 4))
        for angle in stride(from: 0.0, to: 360.0, by: 90.0) {
            let radians = angle * .pi / 180
            let x = rect.midX + cos(radians) * 5
            let y = rect.midY + sin(radians) * 5
            path.appendOval(in: NSRect(x: x - 3, y: y - 3, width: 6, height: 6))
        }
    case 2:
        path.appendOval(in: NSRect(x: rect.midX - 5, y: rect.midY - 5, width: 10, height: 10))
        path.appendOval(in: NSRect(x: rect.midX - 2, y: rect.midY - 2, width: 4, height: 4))
        for angle in stride(from: 0.0, to: 360.0, by: 45.0) {
            let radians = angle * .pi / 180
            path.move(to: CGPoint(x: rect.midX + cos(radians) * 6, y: rect.midY + sin(radians) * 6))
            path.line(to: CGPoint(x: rect.midX + cos(radians) * 8, y: rect.midY + sin(radians) * 8))
        }
    default:
        path.appendOval(in: NSRect(x: rect.minX + 3, y: rect.maxY - 7, width: rect.width - 6, height: 5))
        path.move(to: CGPoint(x: rect.minX + 3, y: rect.maxY - 4.5))
        path.line(to: CGPoint(x: rect.minX + 3, y: rect.minY + 6))
        path.move(to: CGPoint(x: rect.maxX - 3, y: rect.maxY - 4.5))
        path.line(to: CGPoint(x: rect.maxX - 3, y: rect.minY + 6))
        path.appendOval(in: NSRect(x: rect.minX + 3, y: rect.minY + 3, width: rect.width - 6, height: 5))
    }
    path.stroke()
}

func drawRail(_ spec: CaseSpec, rail: NSRect) {
    let activeIndex = ["status", "fans", "settings", "system"].firstIndex(of: spec.view) ?? 0
    for index in 0..<4 {
        let y = rail.maxY - 50 - CGFloat(index) * 44
        let fill = index == activeIndex ? spec.palette.accent.withAlphaComponent(spec.isDark ? 0.15 : 0.11) : NSColor.clear
        rounded(NSRect(x: rail.minX + 5, y: y, width: 40, height: 40), radius: 8, fill: fill)
        drawRailIcon(
            index,
            in: NSRect(x: rail.minX + 15, y: y + 10, width: 20, height: 20),
            color: index == activeIndex ? spec.palette.accent : spec.palette.dim
        )
    }
}

func drawSectionTitle(_ title: String, pill: String?, y: CGFloat, main: NSRect, palette: Palette) {
    text(title, NSRect(x: main.minX + 18, y: y, width: 180, height: 17), 11, palette.text, weight: .bold)
    if let pill {
        let pillRect = NSRect(x: main.maxX - 77, y: y - 1, width: 59, height: 20)
        rounded(pillRect, radius: 7, fill: palette.accent.withAlphaComponent(0.12))
        text(pill, NSRect(x: pillRect.minX, y: pillRect.minY + 5, width: pillRect.width, height: 11), 7.5, palette.accent, weight: .bold, align: .center)
    }
}

func drawAuxiliaryView(_ spec: CaseSpec, main: NSRect) {
    let p = spec.palette
    let left = main.minX + 18
    let width = main.width - 36
    let top = main.maxY - 78

    if spec.view == "fans" {
        drawSectionTitle(label("Ready", "준비 완료", spec), pill: label("Connected", "연결됨", spec), y: top, main: main, palette: p)
        text(label("Automatic control is active", "자동 팬 제어가 활성화됨", spec), NSRect(x: left, y: top - 19, width: width, height: 14), 8.5, p.dim)
        line(from: CGPoint(x: main.minX, y: top - 34), to: CGPoint(x: main.maxX, y: top - 34), color: p.line)

        drawSectionTitle(label("Control mode", "제어 모드", spec), pill: nil, y: top - 62, main: main, palette: p)
        let profiles = spec.isKorean
            ? ["자동", "저소음", "균형", "게임", "성능", "최대"]
            : ["Auto", "Quiet", "Balance", "Game", "Fast", "Max"]
        let segmentY = top - 132
        rounded(NSRect(x: left, y: segmentY, width: width, height: 61), radius: 9, fill: p.panel)
        let segmentWidth = (width - 10) / 3
        for (index, profile) in profiles.enumerated() {
            let column = index % 3
            let row = index / 3
            let x = left + 4 + CGFloat(column) * (segmentWidth + 1)
            let y = segmentY + 31 - CGFloat(row) * 28
            if index == 0 {
                rounded(NSRect(x: x, y: y, width: segmentWidth - 1, height: 25), radius: 6, fill: p.section)
            }
            text(profile, NSRect(x: x, y: y + 7, width: segmentWidth - 1, height: 12), 8, index == 0 ? p.text : p.dim, weight: .semibold, align: .center)
        }

        let guideY = segmentY - 47
        rounded(NSRect(x: left, y: guideY, width: width, height: 39), radius: 7, fill: p.panel, stroke: p.line)
        text(label("macOS Auto", "macOS 자동", spec), NSRect(x: left + 9, y: guideY + 21, width: 150, height: 13), 9.5, p.text, weight: .bold)
        text(
            label("macOS adapts to the workload", "현재 작업에 맞춰 자동 조절", spec),
            NSRect(x: left + 9, y: guideY + 7, width: 190, height: 12),
            8.5,
            p.dim
        )
        let previewHeights: [CGFloat] = [5, 8, 12, 18, 24]
        for (index, height) in previewHeights.enumerated() {
            rounded(
                NSRect(x: main.maxX - 62 + CGFloat(index) * 9, y: guideY + 7, width: 6, height: height),
                radius: 2,
                fill: p.green.withAlphaComponent(0.7)
            )
        }

        let diagnostics = [
            (label("Curve input", "커브 입력", spec), "61°C"),
            (label("Safety hottest", "안전 최고", spec), "64°C"),
            (label("Critical limit", "임계값", spec), "95°C"),
        ]
        let diagY = guideY - 48
        let diagWidth = width / 3
        for (index, item) in diagnostics.enumerated() {
            let x = left + CGFloat(index) * diagWidth
            if index > 0 {
                line(from: CGPoint(x: x, y: diagY), to: CGPoint(x: x, y: diagY + 37), color: p.line)
            }
            text(item.0, NSRect(x: x + 8, y: diagY + 23, width: diagWidth - 16, height: 11), 7.5, p.dim, weight: .medium)
            text(item.1, NSRect(x: x + 8, y: diagY + 6, width: diagWidth - 16, height: 14), 10.5, p.text, weight: .bold)
        }
        line(from: CGPoint(x: main.minX, y: diagY - 10), to: CGPoint(x: main.maxX, y: diagY - 10), color: p.line)

        let fans = [
            (label("Left fan", "왼쪽 팬", spec), "2,440 RPM", 0.34),
            (label("Right fan", "오른쪽 팬", spec), "2,388 RPM", 0.32),
        ]
        for (index, fan) in fans.enumerated() {
            let y = diagY - 54 - CGFloat(index) * 55
            text(fan.0, NSRect(x: left, y: y + 28, width: 130, height: 14), 9.5, p.text, weight: .semibold)
            text(fan.1, NSRect(x: main.maxX - 112, y: y + 28, width: 94, height: 14), 9, p.dim, weight: .semibold, align: .right)
            meter(NSRect(x: left, y: y + 15, width: width, height: 4), value: fan.2, color: p.accent, palette: p)
            text(label("OS automatic", "시스템 자동", spec), NSRect(x: left, y: y - 2, width: width, height: 12), 8, p.dim)
        }
    } else if spec.view == "settings" {
        drawSectionTitle(label("General", "일반", spec), pill: "PeterFan", y: top, main: main, palette: p)
        let loginY = top - 54
        text(label("Start on login", "로그인 시 시작", spec), NSRect(x: left, y: loginY + 14, width: 180, height: 15), 10, p.text, weight: .semibold)
        rounded(NSRect(x: main.maxX - 73, y: loginY + 8, width: 55, height: 25), radius: 7, fill: p.accent.withAlphaComponent(0.13))
        text(label("Enabled", "켜짐", spec), NSRect(x: main.maxX - 73, y: loginY + 15, width: 55, height: 12), 8, p.accent, weight: .bold, align: .center)
        line(from: CGPoint(x: left, y: loginY - 4), to: CGPoint(x: main.maxX - 18, y: loginY - 4), color: p.line)
        let menuY = loginY - 42
        text(label("Menu bar", "메뉴 막대", spec), NSRect(x: left, y: menuY + 8, width: 100, height: 15), 10, p.text, weight: .semibold)
        let menuWidth: CGFloat = 144
        let menuX = main.maxX - menuWidth - 18
        rounded(NSRect(x: menuX, y: menuY + 2, width: menuWidth, height: 27), radius: 7, fill: p.panel)
        let menuLabels = spec.isKorean ? ["숫자", "고양이", "둘 다"] : ["Number", "Cat", "Both"]
        for (index, menuLabel) in menuLabels.enumerated() {
            let segmentWidth = menuWidth / 3
            let x = menuX + CGFloat(index) * segmentWidth
            if index == 2 {
                rounded(NSRect(x: x + 2, y: menuY + 4, width: segmentWidth - 4, height: 23), radius: 5, fill: p.section)
            }
            text(menuLabel, NSRect(x: x, y: menuY + 11, width: segmentWidth, height: 12), 7.5, index == 2 ? p.text : p.dim, weight: .semibold, align: .center)
        }
        text(label("CPU 54% · Running", "CPU 54% · 빠르게", spec), NSRect(x: menuX, y: menuY - 10, width: menuWidth, height: 11), 7.5, p.dim, align: .right)
        line(from: CGPoint(x: left, y: menuY - 18), to: CGPoint(x: main.maxX - 18, y: menuY - 18), color: p.line)
        text(label("Fan control health", "팬 제어 상태", spec), NSRect(x: left, y: menuY - 48, width: 190, height: 15), 10, p.text, weight: .semibold)
        text(label("Ready", "정상", spec), NSRect(x: main.maxX - 80, y: menuY - 48, width: 62, height: 15), 9, p.green, weight: .bold, align: .right)

        line(from: CGPoint(x: main.minX, y: menuY - 70), to: CGPoint(x: main.maxX, y: menuY - 70), color: p.line)
        drawSectionTitle(label("Updates", "업데이트", spec), pill: label("Current", "최신", spec), y: menuY - 98, main: main, palette: p)
        let updateRows = [
            (label("Installed app", "설치된 앱", spec), "v\(version)"),
            (label("Latest signed", "최신 서명 릴리스", spec), "v\(version)"),
            (label("Status", "상태", spec), label("Up to date", "최신 상태", spec)),
        ]
        for (index, item) in updateRows.enumerated() {
            let y = menuY - 135 - CGFloat(index) * 28
            text(item.0, NSRect(x: left, y: y, width: 160, height: 14), 8.5, p.dim)
            text(item.1, NSRect(x: main.maxX - 150, y: y, width: 132, height: 14), 8.5, index == 2 ? p.green : p.text, weight: .semibold, align: .right)
            line(from: CGPoint(x: left, y: y - 8), to: CGPoint(x: main.maxX - 18, y: y - 8), color: p.line)
        }
        let buttonY = menuY - 224
        rounded(NSRect(x: left, y: buttonY, width: 118, height: 30), radius: 7, fill: p.accent.withAlphaComponent(0.15))
        text(label("Check for updates", "업데이트 확인", spec), NSRect(x: left, y: buttonY + 9, width: 118, height: 13), 8.5, p.accent, weight: .bold, align: .center)
    } else {
        drawSectionTitle(label("Hardware", "하드웨어", spec), pill: label("Live", "실시간", spec), y: top, main: main, palette: p)
        let facts = [
            (label("Load average", "로드 평균", spec), "2.18 · 1.92 · 1.74"),
            (label("Power", "소비 전력", spec), "35.4 W"),
            (label("Network rate", "네트워크 속도", spec), "1.4 MB/s"),
            (label("Uptime", "가동 시간", spec), "3d 7h"),
        ]
        let factsTop = top - 31
        for (index, fact) in facts.enumerated() {
            let column = index % 2
            let row = index / 2
            let factWidth = width / 2
            let x = left + CGFloat(column) * factWidth
            let y = factsTop - CGFloat(row) * 37
            if column == 1 {
                line(from: CGPoint(x: x, y: y - 5), to: CGPoint(x: x, y: y + 27), color: p.line)
            }
            text(fact.0, NSRect(x: x + (column == 1 ? 10 : 0), y: y + 13, width: factWidth - 12, height: 11), 7.5, p.dim, weight: .semibold)
            text(fact.1, NSRect(x: x + (column == 1 ? 10 : 0), y: y - 1, width: factWidth - 12, height: 13), 8.5, p.text, weight: .bold)
        }
        line(from: CGPoint(x: left, y: factsTop - 48), to: CGPoint(x: main.maxX - 18, y: factsTop - 48), color: p.line)
        let items: [(String, String, CGFloat, NSColor)] = [
            (label("Storage", "저장공간", spec), "87.2%", 0.87, p.red),
            (label("Battery", "배터리", spec), "98%", 0.98, p.green),
            (label("Network", "네트워크", spec), "1.4 MB/s", 0.42, p.accent),
            (label("Top process", "상위 프로세스", spec), "PeterFan 4.2%", 0.18, p.yellow),
        ]
        for (index, item) in items.enumerated() {
            let y = top - 142 - CGFloat(index) * 61
            text(item.0, NSRect(x: left, y: y + 38, width: 150, height: 15), 10, p.text, weight: .semibold)
            text(item.1, NSRect(x: main.maxX - 140, y: y + 38, width: 122, height: 15), 10, p.text, weight: .bold, align: .right)
            meter(NSRect(x: left, y: y + 22, width: width, height: 4), value: item.2, color: item.3, palette: p)
            if index < items.count - 1 {
                line(from: CGPoint(x: main.minX, y: y - 6), to: CGPoint(x: main.maxX, y: y - 6), color: p.line)
            }
        }
    }
}

func drawCase(_ spec: CaseSpec, origin: CGPoint) {
    let p = spec.palette
    rounded(NSRect(origin: origin, size: cellSize), radius: 16, fill: p.background, stroke: p.line)
    text(spec.title, NSRect(x: origin.x + 18, y: origin.y + cellSize.height - 32, width: 160, height: 18), 11, p.dim, weight: .semibold)
    text("v\(version)", NSRect(x: origin.x + cellSize.width - 92, y: origin.y + cellSize.height - 32, width: 72, height: 18), 11, p.accent, weight: .bold, align: .right)

    let popover = NSRect(x: origin.x + 18, y: origin.y + 18, width: 404, height: 456)
    let railWidth: CGFloat = 50
    let main = NSRect(x: popover.minX, y: popover.minY, width: popover.width - railWidth, height: popover.height)
    let rail = NSRect(x: main.maxX, y: popover.minY, width: railWidth, height: popover.height)
    rounded(popover, radius: 12, fill: p.panel, stroke: p.line)
    NSGraphicsContext.saveGraphicsState()
    NSBezierPath(roundedRect: popover, xRadius: 12, yRadius: 12).addClip()
    p.section.setFill()
    NSRect(x: main.minX, y: main.minY, width: main.width, height: main.height).fill()
    NSGraphicsContext.restoreGraphicsState()
    p.line.setFill()
    NSRect(x: rail.minX, y: rail.minY, width: 1, height: rail.height).fill()

    let viewTitle = spec.view == "status"
        ? "PeterFan"
        : (spec.view == "fans"
            ? label("Fans", "팬", spec)
            : (spec.view == "settings" ? label("Settings", "설정", spec) : label("System", "시스템", spec)))
    text(viewTitle, NSRect(x: main.minX + 18, y: main.maxY - 34, width: 140, height: 18), 15, p.text, weight: .bold)
    let ranges = ["2m", "1h", "1d"]
    if spec.view == "status" {
        for (index, range) in ranges.enumerated() {
            let x = main.maxX - 104 + CGFloat(index) * 29
            let selected = index == 0
            rounded(
                NSRect(x: x, y: main.maxY - 36, width: 26, height: 21),
                radius: 6,
                fill: selected ? p.accent.withAlphaComponent(spec.isDark ? 0.13 : 0.10) : p.panel
            )
            text(range, NSRect(x: x, y: main.maxY - 31, width: 26, height: 11), 8, selected ? p.accent : p.dim, weight: .bold, align: .center)
        }
    }
    line(from: CGPoint(x: main.minX, y: main.maxY - 50), to: CGPoint(x: main.maxX, y: main.maxY - 50), color: p.line)

    if spec.view != "status" {
        drawAuxiliaryView(spec, main: main)
        drawRail(spec, rail: rail)
        return
    }

    let healthY = main.maxY - 91
    rounded(NSRect(x: main.minX, y: healthY, width: main.width, height: 40), radius: 0, fill: p.panel)
    rounded(NSRect(x: main.minX + 18, y: healthY + 16, width: 9, height: 9), radius: 3, fill: p.green)
    text(label("Normal", "정상", spec), NSRect(x: main.minX + 37, y: healthY + 20, width: 64, height: 14), 10.5, p.green, weight: .bold)
    text(
        label("CPU avg 61°C · CPU 57% · fans 3865 RPM", "CPU 평균 61°C · CPU 57% · 팬 3865 RPM", spec),
        NSRect(x: main.minX + 96, y: healthY + 20, width: main.width - 114, height: 14),
        8.5,
        p.dim
    )
    line(from: CGPoint(x: main.minX, y: healthY), to: CGPoint(x: main.maxX, y: healthY), color: p.line)

    let summary: [(String, String, CGFloat, NSColor)] = [
        (label("CPU", "CPU", spec), "57%", 0.57, p.green),
        (label("Memory", "메모리", spec), "73%", 0.73, p.accent),
        (label("CPU temp", "CPU 온도", spec), "61°C", 0.61, p.yellow),
        (label("Fans", "팬", spec), "3865", 0.52, p.accent),
    ]
    let summaryX = main.minX + 18
    let summaryY = main.maxY - 163
    let summaryWidth = (main.width - 36) / 4
    for (index, item) in summary.enumerated() {
        let x = summaryX + CGFloat(index) * summaryWidth
        if index > 0 {
            line(from: CGPoint(x: x, y: summaryY + 4), to: CGPoint(x: x, y: summaryY + 48), color: p.line)
        }
        text(item.0, NSRect(x: x + 9, y: summaryY + 38, width: summaryWidth - 18, height: 12), 8, p.dim, weight: .semibold)
        text(item.1, NSRect(x: x + 9, y: summaryY + 18, width: summaryWidth - 18, height: 19), 14, p.text, weight: .bold)
        meter(NSRect(x: x + 9, y: summaryY + 10, width: summaryWidth - 18, height: 3), value: item.2, color: item.3, palette: p)
    }
    line(from: CGPoint(x: main.minX, y: summaryY - 8), to: CGPoint(x: main.maxX, y: summaryY - 8), color: p.line)

    let contentX = main.minX + 18
    let contentWidth = main.width - 36
    text(label("CPU activity", "CPU 사용량", spec), NSRect(x: contentX, y: summaryY - 36, width: 130, height: 16), 10.5, p.text, weight: .semibold)
    text("57%", NSRect(x: main.maxX - 72, y: summaryY - 36, width: 54, height: 16), 12, p.text, weight: .bold, align: .right)
    let barsY = summaryY - 65
    for i in 0..<18 {
        let color = i < 4 ? p.red : (i < 12 ? p.green : p.yellow)
        let height: CGFloat = 8 + CGFloat((i * 7) % 16)
        rounded(NSRect(x: contentX + CGFloat(i) * 17, y: barsY, width: 12, height: height), radius: 2, fill: color)
    }
    sparkline(NSRect(x: contentX, y: barsY - 40, width: contentWidth, height: 32), values: [0.2, 0.18, 0.22, 0.19, 0.28, 0.26, 0.34, 0.33, 0.50, 0.54, 0.51, 0.56], palette: p)
    line(from: CGPoint(x: main.minX, y: barsY - 49), to: CGPoint(x: main.maxX, y: barsY - 49), color: p.line)

    let rows: [(String, String, CGFloat, NSColor)] = [
        (label("Memory", "메모리", spec), "73.3%", 0.73, p.yellow),
        (label("CPU avg temp", "CPU 평균 온도", spec), "61°C", 0.61, p.yellow),
    ]
    for (index, row) in rows.enumerated() {
        let y = barsY - 101 - CGFloat(index) * 67
        text(row.0, NSRect(x: contentX, y: y + 29, width: 160, height: 17), 10.5, p.text, weight: .semibold)
        text(row.1, NSRect(x: main.maxX - 90, y: y + 29, width: 72, height: 17), 12, p.text, weight: .bold, align: .right)
        meter(NSRect(x: contentX, y: y + 15, width: contentWidth, height: 4), value: row.2, color: row.3, palette: p)
        if index == 0 {
            line(from: CGPoint(x: main.minX, y: y - 5), to: CGPoint(x: main.maxX, y: y - 5), color: p.line)
        }
    }

    text(label("All sensors · 18", "전체 센서 · 18", spec), NSRect(x: contentX, y: main.minY + 17, width: contentWidth, height: 16), 10, p.dim, weight: .semibold)

    drawRail(spec, rail: rail)
}

image.lockFocus()
c(18, 20, 24).setFill()
NSRect(origin: .zero, size: size).fill()
text("PeterFan Popover Visual QA", NSRect(x: margin, y: size.height - 30, width: 320, height: 18), 14, c(234, 238, 246), weight: .bold)
text("status · fans · settings · system · dark/light · English/Korean", NSRect(x: margin + 230, y: size.height - 30, width: 520, height: 18), 11, c(151, 161, 176))

let cases = [
    CaseSpec(title: "Dark / Status / English", view: "status", language: "en", palette: dark, isKorean: false, isDark: true),
    CaseSpec(title: "Dark / Fans / Korean", view: "fans", language: "ko", palette: dark, isKorean: true, isDark: true),
    CaseSpec(title: "Light / Settings / English", view: "settings", language: "en", palette: light, isKorean: false, isDark: false),
    CaseSpec(title: "Light / System / Korean", view: "system", language: "ko", palette: light, isKorean: true, isDark: false),
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
