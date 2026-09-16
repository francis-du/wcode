// Full production DOM regression, offline; generate with browser.cjs first.
import AppKit
import Foundation
import WebKit

final class BrowserAudit: NSObject, WKNavigationDelegate {
    let web: WKWebView
    let widths = [320,375,720,900,1024,1240,1280,1440,1461,1597,1676,1920]
    var scenarios: [(Int,String,String,String)] = []
    var reports: [[String:Any]] = []
    var index = 0
    let check = #"""
    (()=>{
      const errors=[], diagnostics=[], r=e=>e.getBoundingClientRect(), visible=e=>e.getClientRects().length>0;
      const describe=el=>{
        if(!el)return null;
        const b=r(el),s=getComputedStyle(el);
        return {tag:el.tagName,id:el.id,className:el.className,left:b.left,right:b.right,top:b.top,bottom:b.bottom,width:b.width,height:b.height,scrollWidth:el.scrollWidth,clientWidth:el.clientWidth,display:s.display,position:s.position,minWidth:s.minWidth,maxWidth:s.maxWidth,gridTemplateColumns:s.gridTemplateColumns,gridTemplateRows:s.gridTemplateRows,margin:s.margin,padding:s.padding,overflow:s.overflow,transform:s.transform};
      };
      const check=(ok,label,el=null,parent=null)=>{
        if(ok)return;
        errors.push(label);
        diagnostics.push({label,element:describe(el),parent:describe(parent)});
      };
      check(window.__layoutReady===true,'production boot failed');
      check(document.documentElement.scrollWidth<=innerWidth+1,'page overflow',document.documentElement);
      check(document.querySelectorAll('[role="tab"][aria-selected="true"]').length===1,'tab selection');
      for(const selector of ['.bar-row','.frontier-row','.evidence-ledger-head','.evidence-ledger-row','.evidence-inspector-identity','.proof-signal-card','.workspace-context','.global-bar']){
        document.querySelectorAll(selector).forEach((el,i)=>{
          if(!visible(el))return;
          check(el.scrollWidth<=el.clientWidth+1,selector+' content overflow '+i,el,el.parentElement);
          if(selector==='.bar-row'){
            const a=r(el.querySelector('.bar-name')),b=r(el.querySelector('.bar-val')),c=r(el.querySelector('.bar-track'));
            check(a.right<=b.left+1,'stat overlap',el);check(c.top>=Math.max(a.bottom,b.bottom)-1,'bar overlap',el);
          }
        });
      }
      document.querySelectorAll('.adaptive-cards,.code-distribution,.proof-main-grid,.proof-signal-grid').forEach(grid=>{
        if(!visible(grid))return;const parent=r(grid),children=[...grid.children].filter(visible);
        children.forEach((child,i)=>{const a=r(child);check(a.left>=parent.left-1&&a.right<=parent.right+1&&a.bottom<=parent.bottom+1,'grid child outside '+grid.className,child,grid);
          children.slice(i+1).forEach(other=>{const b=r(other);check(Math.min(a.right,b.right)-Math.max(a.left,b.left)<=1||Math.min(a.bottom,b.bottom)-Math.max(a.top,b.top)<=1,'grid overlap '+grid.className,child,other);});});
      });
      if(state.workspaceTab==='proof'){
        check(document.querySelectorAll('[data-evidence-key]').length===32,'missing populated ledger');
        check(!document.querySelector('.evidence-inspector-section .inspector-chip.good'),'failed proof green');
      }
      const header=document.querySelector('.global-bar');
      const headerChildren=header&&header.scrollWidth>header.clientWidth+1?[...header.querySelectorAll('*')].filter(el=>visible(el)&&(r(el).right>r(header).right+1||r(el).left<r(header).left-1)).slice(0,12).map(describe):[];
      return {width:innerWidth,language:state.language,theme:state.theme,tab:state.workspaceTab,errors,diagnostics,headerChildren};
    })()
    """#
    override init(){
        let config=WKWebViewConfiguration();config.websiteDataStore = .nonPersistent()
        web=WKWebView(frame:NSRect(x:0,y:0,width:1597,height:900),configuration:config)
        super.init();web.navigationDelegate=self
        for width in widths {for lang in ["en","zh-CN"] {for theme in ["dark","light"] {for tab in ["proof","overview"] {scenarios.append((width,lang,theme,tab))}}}}
    }
    func start(){let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath);let file=root.appendingPathComponent("target/wcode-browser-fixture.html");web.loadFileURL(file,allowingReadAccessTo:root)}
    func webView(_ webView:WKWebView,didFinish navigation:WKNavigation!){next()}
    func webView(_ webView:WKWebView,didFail navigation:WKNavigation!,withError error:Error){fputs("\(error)\n",stderr);exit(2)}
    func next(){
        guard index<scenarios.count else {
            let failures=reports.reduce(0){$0+(($1["errors"] as? [String])?.count ?? 1)}
            let data=try! JSONSerialization.data(withJSONObject:["suite":"full-browser-adversarial","failures":failures,"cases":reports.count,"results":reports],options:[.prettyPrinted,.sortedKeys])
            try! data.write(to:URL(fileURLWithPath:"target/wcode-browser-audit.json"));print(String(data:data,encoding:.utf8)!);exit(failures==0 ? 0:1)
        }
        let (width,lang,theme,tab)=scenarios[index];index+=1
        web.setFrameSize(NSSize(width:width,height:900));web.layoutSubtreeIfNeeded()
        let setup="state.language='\(lang)';state.theme='\(theme)';applyTheme();applyLanguage();activateWorkspaceTab('\(tab)');window.scrollTo(0,0);"
        web.evaluateJavaScript(setup){_,error in
            if let error {fputs("\(error)\n",stderr);exit(2)}
            DispatchQueue.main.asyncAfter(deadline:.now()+0.12){self.web.evaluateJavaScript(self.check){value,error in
                guard error==nil,let report=value as? [String:Any] else {fputs("browser check failed\n",stderr);exit(2)}
                self.reports.append(report);self.next()
            }}
        }
    }
}
let app=NSApplication.shared;app.setActivationPolicy(.prohibited)
let audit=BrowserAudit();DispatchQueue.main.asyncAfter(deadline:.now()+90){fputs("browser audit timed out\n",stderr);exit(2)}
audit.start();app.run()
