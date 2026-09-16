// Full production DOM regression, offline; generate with browser.cjs first.
import AppKit
import Foundation
import WebKit

final class BrowserAudit: NSObject, WKNavigationDelegate {
    let web: WKWebView
    let window: NSWindow
    let widths = [320,375,720,900,1024,1240,1280,1440,1461,1597,1676,1920]
    var scenarios: [(Int,String,String,String)] = []
    var reports: [[String:Any]] = []
    var index = 0
    let check = #"""
    (()=>{
      const errors=[],diagnostics=[],r=e=>e.getBoundingClientRect(),visible=e=>e.getClientRects().length>0;
      const describe=el=>{
        const box=r(el),style=getComputedStyle(el);
        return {tag:el.tagName,id:el.id,className:String(el.className),
          rect:{left:box.left,top:box.top,right:box.right,bottom:box.bottom,width:box.width,height:box.height},
          clientWidth:el.clientWidth,scrollWidth:el.scrollWidth,clientHeight:el.clientHeight,scrollHeight:el.scrollHeight,
          display:style.display,position:style.position,top:style.top,bottom:style.bottom,
          minWidth:style.minWidth,width:style.width,height:style.height,alignSelf:style.alignSelf,
          columns:style.gridTemplateColumns,rows:style.gridTemplateRows,gap:style.gap};
      };
      const check=(ok,label,details)=>{if(!ok){errors.push(label);diagnostics.push({label,...details});}};
      check(window.__layoutReady===true,'production boot failed');
      check(innerWidth===window.__expectedAuditWidth,'viewport resize not applied',
        {expected:window.__expectedAuditWidth,actual:innerWidth});
      const pageFits=document.documentElement.scrollWidth<=innerWidth+1;
      check(pageFits,'page overflow',pageFits?undefined:{page:describe(document.documentElement),
        outside:[...document.body.querySelectorAll('*')].filter(el=>visible(el)&&(r(el).left < -1||r(el).right>innerWidth+1)).slice(0,24).map(describe)});
      check(document.querySelectorAll('[role="tab"][aria-selected="true"]').length===1,'tab selection');
      for(const selector of ['.bar-row','.frontier-row','.evidence-ledger-head','.evidence-ledger-row','.evidence-inspector-identity','.proof-signal-card','.workspace-context','.global-bar','.evidence-inspector-card','.evidence-inspector-section','.evidence-inspector-card .inspector-chip-list','.evidence-inspector-section pre']){
        document.querySelectorAll(selector).forEach((el,i)=>{
          if(!visible(el))return;
          const fits=el.scrollWidth<=el.clientWidth+1;
          check(fits,selector+' content overflow '+i,fits?undefined:{element:describe(el),children:[...el.children].filter(visible).map(describe)});
          if(selector==='.bar-row'){
            const a=r(el.querySelector('.bar-name')),b=r(el.querySelector('.bar-val')),c=r(el.querySelector('.bar-track'));
            check(a.right<=b.left+1,'stat overlap');check(c.top>=Math.max(a.bottom,b.bottom)-1,'bar overlap');
          }
        });
      }
      document.querySelectorAll('.adaptive-cards,.code-distribution,.proof-main-grid,.proof-signal-grid').forEach(grid=>{
        if(!visible(grid))return;const parent=r(grid),children=[...grid.children].filter(visible);
        children.forEach((child,i)=>{
          const a=r(child),fits=a.left>=parent.left-1&&a.right<=parent.right+1&&a.bottom<=parent.bottom+1;
          check(fits,'grid child outside '+grid.className,fits?undefined:{grid:describe(grid),child:describe(child),index:i});
          children.slice(i+1).forEach(other=>{const b=r(other);check(Math.min(a.right,b.right)-Math.max(a.left,b.left)<=1||Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)<=1,'grid overlap '+grid.className);});
        });
      });
      if(state.workspaceTab==='proof'){
        check(document.querySelectorAll('[data-evidence-key]').length===32,'missing populated ledger');
        check(!document.querySelector('.evidence-inspector-section .inspector-chip.good'),'failed proof green');
      }
      return {width:innerWidth,language:state.language,theme:state.theme,tab:state.workspaceTab,
        scrollX,scrollY,devicePixelRatio,fontStatus:document.fonts.status,
        coarsePointer:matchMedia('(pointer:coarse)').matches,
        gutter:getComputedStyle(document.documentElement).getPropertyValue('--page-gutter-x'),
        media:[1680,1460,1240,900,720,520].map(width=>({width,matches:matchMedia(`(max-width:${width}px)`).matches})),errors,diagnostics};
    })()
    """#
    override init(){
        let config=WKWebViewConfiguration();config.websiteDataStore = .nonPersistent()
        web=WKWebView(frame:NSRect(x:0,y:0,width:1597,height:900),configuration:config)
        window=NSWindow(contentRect:NSRect(x:0,y:0,width:1597,height:900),styleMask:.borderless,backing:.buffered,defer:false)
        super.init();web.navigationDelegate=self
        // Host the renderer so resizing exercises the actual view hierarchy.
        window.isReleasedWhenClosed=false
        window.contentView=web
        web.autoresizingMask=[.width,.height]
        window.orderFront(nil)
        for width in widths {for lang in ["en","zh-CN"] {for theme in ["dark","light"] {for tab in ["proof","overview"] {scenarios.append((width,lang,theme,tab))}}}}
    }
    func start(){let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath);let file=root.appendingPathComponent("target/wcode-browser-fixture.html");web.loadFileURL(file,allowingReadAccessTo:root)}
    func webView(_ webView:WKWebView,didFinish navigation:WKNavigation!){next()}
    func webView(_ webView:WKWebView,didFail navigation:WKNavigation!,withError error:Error){fputs("\(error)\n",stderr);exit(2)}
    func next(){
        guard index<scenarios.count else {
            let failures=reports.reduce(0){$0+(($1["errors"] as? [String])?.count ?? 1)}
            let failedCases=reports.filter{!(($0["errors"] as? [String])?.isEmpty ?? false)}.count
            let data=try! JSONSerialization.data(withJSONObject:["suite":"full-browser-adversarial","failures":failures,"failed_cases":failedCases,"cases":reports.count,"results":reports],options:[.prettyPrinted,.sortedKeys])
            try! data.write(to:URL(fileURLWithPath:"target/wcode-browser-audit.json"));print(String(data:data,encoding:.utf8)!);exit(failures==0 ? 0:1)
        }
        let (width,lang,theme,tab)=scenarios[index];index+=1
        window.setContentSize(NSSize(width:width,height:900))
        web.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        let setup="window.__expectedAuditWidth=\(width);state.language='\(lang)';state.theme='\(theme)';applyTheme();applyLanguage();activateWorkspaceTab('\(tab)');window.scrollTo(0,0);"
        web.evaluateJavaScript(setup){_,error in
            if let error {fputs("\(error)\n",stderr);exit(2)}
            DispatchQueue.main.asyncAfter(deadline:.now()+0.12){self.web.evaluateJavaScript(self.check){value,error in
                guard error==nil,let report=value as? [String:Any] else {fputs("browser check failed\n",stderr);exit(2)}
                self.reports.append(report);self.next()
            }}
        }
    }
}
let app=NSApplication.shared;app.setActivationPolicy(.accessory)
let audit=BrowserAudit();DispatchQueue.main.asyncAfter(deadline:.now()+90){fputs("browser audit timed out\n",stderr);exit(2)}
audit.start();app.run()
