import AppKit

let version = Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? "unknown"
let banner = "omnipackage-macos-hello-gui \(version)"

if CommandLine.arguments.contains("--version") {
    print(banner)
    exit(0)
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    var window: NSWindow?

    func applicationDidFinishLaunching(_ notification: Notification) {
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 380, height: 120), styleMask: [.titled, .closable, .miniaturizable], backing: .buffered, defer: false)
        window.title = "Omnipackage Hello"
        let label = NSTextField(labelWithString: banner)
        label.font = .systemFont(ofSize: 18)
        label.frame = NSRect(x: 20, y: 48, width: 340, height: 26)
        window.contentView?.addSubview(label)
        window.center()
        window.makeKeyAndOrderFront(nil)
        self.window = window
        NSApp.activate(ignoringOtherApps: true)
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        true
    }
}

let app = NSApplication.shared
let delegate = AppDelegate()
app.delegate = delegate
app.setActivationPolicy(.regular)
app.run()
