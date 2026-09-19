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
    var totalCases = 0
    var finished = false
    var phase = "initializing"
    var timeoutSeconds: Double = 90
    let check = #"""
    (()=>{
      const errors=[], diagnostics=[], r=e=>e.getBoundingClientRect(), visible=e=>e.getClientRects().length>0;
      const describe=el=>{
        if(!el)return null;
        const b=r(el),s=getComputedStyle(el);
        return {tag:el.tagName,id:el.id,className:el.className,left:b.left,right:b.right,top:b.top,bottom:b.bottom,width:b.width,height:b.height,scrollWidth:el.scrollWidth,clientWidth:el.clientWidth,scrollHeight:el.scrollHeight,clientHeight:el.clientHeight,display:s.display,position:s.position,minWidth:s.minWidth,maxWidth:s.maxWidth,gridTemplateColumns:s.gridTemplateColumns,gridTemplateRows:s.gridTemplateRows,margin:s.margin,padding:s.padding,overflow:s.overflow,transform:s.transform};
      };
      const check=(ok,label,el=null,parent=null)=>{
        if(ok)return;
        errors.push(label);
        diagnostics.push({label,element:describe(el),parent:describe(parent)});
      };
      check(window.__layoutReady===true,'production boot failed');
      check(document.documentElement.scrollWidth<=innerWidth+1,'page overflow',document.documentElement);
      check(document.querySelectorAll('[role="tab"][aria-selected="true"]').length===1,'tab selection');
      for(const selector of ['.bar-row','.frontier-row','.evidence-ledger-head','.evidence-ledger-row','.evidence-inspector-identity','.proof-signal-card','.workspace-context','.global-bar','.evidence-inspector-card','.evidence-inspector-section','.evidence-inspector-card .inspector-chip-list','.evidence-inspector-section pre']){
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
      if(state.workspaceTab==='architecture'&&state.architectureView==='codegraph'){
        const section=document.getElementById('codeGraphSection');
        check(section&&visible(section),'code graph hidden',section);
        check(document.querySelectorAll('[data-code-node]').length>=3,'missing populated code graph',section);
        check((document.getElementById('codeGraphInspector')?.textContent||'').includes('renderProject'),'missing code graph inspector focus',document.getElementById('codeGraphInspector'));
      }
      const header=document.querySelector('.global-bar');
      const headerChildren=header&&header.scrollWidth>header.clientWidth+1?[...header.querySelectorAll('*')].filter(el=>visible(el)&&(r(el).right>r(header).right+1||r(el).left<r(header).left-1)).slice(0,12).map(describe):[];
      return {width:innerWidth,language:state.language,theme:state.theme,tab:state.workspaceTab,errors,diagnostics,headerChildren,
        scrollX,scrollY,devicePixelRatio,fontStatus:document.fonts.status,
        coarsePointer:matchMedia('(pointer:coarse)').matches,
        gutter:getComputedStyle(document.documentElement).getPropertyValue('--page-gutter-x'),
        media:[1680,1460,1240,900,720,520].map(width=>({width,matches:matchMedia(`(max-width:${width}px)`).matches}))};
    })()
    """#
    override init(){
        let config=WKWebViewConfiguration();config.websiteDataStore = .nonPersistent()
        let frame=NSRect(x:0,y:0,width:1597,height:900)
        web=WKWebView(frame:frame,configuration:config)
        super.init();web.navigationDelegate=self
        for width in widths {for lang in ["en","zh-CN"] {for theme in ["dark","light"] {for tab in ["proof","overview"] {scenarios.append((width,lang,theme,tab))}}}}
        for width in [320,720,1024,1440] {for lang in ["en","zh-CN"] {for theme in ["dark","light"] {scenarios.append((width,lang,theme,"codegraph"))}}}
        totalCases=scenarios.count
        if let option=CommandLine.arguments.first(where:{$0.hasPrefix("--timeout=")}) {
            let value=String(option.dropFirst("--timeout=".count))
            if let seconds=Double(value),seconds>=1 { timeoutSeconds=seconds }
        }
        if let option=CommandLine.arguments.first(where:{$0.hasPrefix("--cases=")}) {
            let value=String(option.dropFirst("--cases=".count))
            let parts=value.split(separator:"-",omittingEmptySubsequences:false)
            guard parts.count==2,
                  let start=Int(parts[0]),let end=Int(parts[1]),
                  start>=1,end<=scenarios.count,start<=end
            else {
                fputs("--cases must use START-END within 1-\(scenarios.count)\n",stderr)
                exit(2)
            }
            scenarios=Array(scenarios[(start-1)..<end])
        }
    }
    func start(){
        phase="navigation"
        let root=URL(fileURLWithPath:FileManager.default.currentDirectoryPath)
        let fixture=root.appendingPathComponent("target/wcode-browser-fixture.html")
        guard FileManager.default.fileExists(atPath:fixture.path) else {fputs("Run browser.cjs first\n",stderr);exit(2)}
        web.loadFileURL(fixture,allowingReadAccessTo:fixture.deletingLastPathComponent())
    }
    func webView(_ webView:WKWebView,didFinish navigation:WKNavigation!){phase="scenario";next()}
    func webView(_ webView:WKWebView,didFail navigation:WKNavigation!,withError error:Error){finish("Navigation failed: \(error)")}
    func webView(_ webView:WKWebView,didFailProvisionalNavigation navigation:WKNavigation!,withError error:Error){finish("Initial navigation failed: \(error)")}
    func finish(_ reason:String? = nil){
        guard !finished else{return};finished=true
        let failures=reports.reduce(0){$0+(($1["errors"] as? [String])?.count ?? 1)} + (reason == nil && reports.count == scenarios.count ? 0:1)
        let failedCases=reports.filter{!(($0["errors"] as? [String])?.isEmpty ?? false)}.count
        let sharded=CommandLine.arguments.contains(where:{$0.hasPrefix("--cases=")})
        let report:[String:Any]=["suite":sharded ? "full-browser-adversarial-shard" : "full-browser-adversarial","failures":failures,"failed_cases":failedCases,"cases":reports.count,"expected_cases":scenarios.count,"total_cases":totalCases,"results":reports,"runner_error":reason ?? ""]
        do {
            let data=try JSONSerialization.data(withJSONObject:report,options:[.prettyPrinted,.sortedKeys])
            try data.write(to:URL(fileURLWithPath:"target/wcode-browser-audit.json"),options:.atomic)
            print(String(data:data,encoding:.utf8)!)
        } catch {fputs("Cannot persist browser audit: \(error)\n",stderr);exit(2)}
        exit(failures==0 ? 0:1)
    }
    func next(){
        guard !finished else{return}
        guard index<scenarios.count else{finish();return}
        let (width,lang,theme,tab)=scenarios[index];index+=1
        phase="setup-\(index)"
        fputs("WebKit case \(index)/\(scenarios.count): \(width) \(lang) \(theme) \(tab)\n",stderr)
        web.setFrameSize(NSSize(width:width,height:900));web.layoutSubtreeIfNeeded()
        let setup="""
        (()=>{state.language='\(lang)';state.theme='\(theme)';applyTheme();applyLanguage();if('\(tab)'==='codegraph'){state.architectureView='codegraph';activateWorkspaceTab('architecture');renderArchitecture();}else{activateWorkspaceTab('\(tab)');}window.scrollTo(0,0);return innerWidth;})()
        """
        web.evaluateJavaScript(setup){value,error in
            if let error {self.finish("Browser setup failed: \(error)");return}
            guard let actual=value as? NSNumber,actual.intValue==width else{self.finish("Requested viewport \(width), received \(String(describing:value))");return}
            self.web.layoutSubtreeIfNeeded()
            DispatchQueue.main.asyncAfter(deadline:.now()+0.15){
                self.phase="check-\(self.index)"
                self.web.evaluateJavaScript(self.check){value,error in
                    guard error==nil,let report=value as? [String:Any] else{self.finish("Browser check failed: \(String(describing:error))");return}
                    self.reports.append(report);self.next()
                }
            }
        }
    }
}
let app=NSApplication.shared;app.setActivationPolicy(.prohibited)
let audit=BrowserAudit()
DispatchQueue.main.asyncAfter(deadline:.now()+audit.timeoutSeconds){audit.finish("Browser audit timed out in \(audit.phase) after \(audit.reports.count)/\(audit.scenarios.count) cases")}
audit.start();app.run()