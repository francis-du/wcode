// macOS real-engine layout check. Run layout.cjs first to export shipped CSS
// and production renderer HTML, then: swift tests/unit/ui/layout_webkit.swift
import AppKit
import Foundation
import WebKit

final class LayoutRunner: NSObject, WKNavigationDelegate {
    let widths: [CGFloat] = [375, 720, 900, 1024, 1240, 1280, 1440, 1461, 1500, 1597, 1676, 1920]
    let webView: WKWebView
    var index = 0
    var failures = 0
    var reports: [[String: Any]] = []
    let script = #"""
    (() => {
      const errors = [], rect = e => e.getBoundingClientRect();
      const check = (ok, name) => { if (!ok) errors.push(name); };
      check(document.documentElement.scrollWidth <= innerWidth + 1, 'page horizontal overflow');
      document.querySelectorAll('.bar-row').forEach((row,i) => {
        const name=row.querySelector('.bar-name'), value=row.querySelector('.bar-val'), track=row.querySelector('.bar-track');
        check(rect(name).right <= rect(value).left+1, 'stat label/value overlap '+i);
        check(rect(value).right <= rect(row).right+1, 'stat value outside row '+i);
        check(rect(track).top >= Math.max(rect(name).bottom,rect(value).bottom)-1, 'stat track overlap '+i);
        check(row.scrollWidth <= row.clientWidth+1, 'stat row text overflow '+i);
      });
      document.querySelectorAll('.code-distribution,.adaptive-cards,.proof-signal-grid').forEach((grid,i) => {
        const parent=rect(grid), children=[...grid.children];
        check(children.length>0, 'missing fixture children '+i);
        children.forEach((child,j) => {
          const r=rect(child);
          check(r.left>=parent.left-1 && r.right<=parent.right+1 && r.bottom<=parent.bottom+1, 'grid child outside '+i+':'+j);
          children.slice(j+1).forEach(other=>{const b=rect(other);check(Math.min(r.right,b.right)-Math.max(r.left,b.left)<=1 || Math.min(r.bottom,b.bottom)-Math.max(r.top,b.top)<=1,'grid children overlap '+i);});
        });
      });
      document.querySelectorAll('[data-layout-adaptive]').forEach((section,i)=>{
        const grid=section.querySelector('.adaptive-cards'), note=section.querySelector('.card-gap');
        check(rect(note).top>=rect(grid).bottom-1,'preview note overlaps cards '+i);
      });
      document.querySelectorAll('.frontier-row,.evidence-ledger-head').forEach((row,i)=>{
        check(row.scrollWidth<=row.clientWidth+1,'frontier/ledger overflow '+i);
      });
      return {width:innerWidth, errors, stats:document.querySelectorAll('.bar-row').length};
    })()
    """#

    override init() {
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .nonPersistent()
        webView = WKWebView(frame: NSRect(x: 0, y: 0, width: 1597, height: 1800), configuration: config)
        super.init()
        webView.navigationDelegate = self
    }
    func start() {
        let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
        let fixture = root.appendingPathComponent("target/wcode-layout-fixture.html")
        guard FileManager.default.fileExists(atPath: fixture.path) else { fputs("Run layout.cjs first\n", stderr); exit(2) }
        webView.loadFileURL(fixture, allowingReadAccessTo: fixture.deletingLastPathComponent())
    }
    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) { next() }
    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fputs("\(error)\n", stderr); exit(2) }
    func next() {
        guard index < widths.count else {
            let data = try! JSONSerialization.data(withJSONObject: ["suite":"webkit-layout", "failures":failures, "results":reports], options: [.prettyPrinted,.sortedKeys])
            try? data.write(to: URL(fileURLWithPath:"target/wcode-webkit-layout.json"))
            print(String(data:data,encoding:.utf8)!)
            exit(failures == 0 ? 0 : 1)
        }
        let width = widths[index]; index += 1
        webView.setFrameSize(NSSize(width: width, height: 1800))
        webView.layoutSubtreeIfNeeded()
        DispatchQueue.main.asyncAfter(deadline:.now()+0.15) {
            self.webView.evaluateJavaScript(self.script) { value,error in
                if let error { fputs("\(error)\n",stderr); exit(2) }
                guard let report=value as? [String:Any], let errors=report["errors"] as? [String] else {exit(2)}
                self.reports.append(report); self.failures += errors.count
                self.next()
            }
        }
    }
}
let app = NSApplication.shared
app.setActivationPolicy(.prohibited)
let runner=LayoutRunner()
DispatchQueue.main.asyncAfter(deadline:.now()+45) { fputs("WebKit layout timeout\n",stderr); exit(2) }
runner.start()
app.run()
