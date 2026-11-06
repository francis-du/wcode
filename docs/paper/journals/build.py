"""Build all six review manuscripts as searchable PDFs with embedded fonts."""
from pathlib import Path
import hashlib, html, json, re
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer, Table, TableStyle, KeepTogether, Flowable, PageBreak
from reportlab.lib.styles import getSampleStyleSheet, ParagraphStyle
from reportlab.lib import colors
from reportlab.lib.enums import TA_JUSTIFY
from reportlab.pdfbase import pdfmetrics
from reportlab.pdfbase.ttfonts import TTFont
from pypdf import PdfReader
HERE=Path(__file__).resolve().parent
OUT=HERE/"output"/"pdf"
FONT=Path("/System/Library/Fonts/Supplemental")
FONTS={"Roman":"Times New Roman.ttf","Bold":"Times New Roman Bold.ttf",
       "Italic":"Times New Roman Italic.ttf","Sans":"Arial.ttf","SansBold":"Arial Bold.ttf",
       "CJK":"Arial Unicode.ttf","Mono":"Courier New.ttf"}
for name,filename in FONTS.items():pdfmetrics.registerFont(TTFont(name,str(FONT/filename)))
pdfmetrics.registerFontFamily("Roman",normal="Roman",bold="Bold",italic="Italic",boldItalic="Bold")
pdfmetrics.registerFontFamily("CJK",normal="CJK",bold="CJK",italic="CJK",boldItalic="CJK")
INK=colors.HexColor("#17242d");ACCENT=colors.HexColor("#28566b")
PALE=colors.HexColor("#edf2f4");GRAY=colors.HexColor("#52616b")
def inline(s):
    s=html.escape(s,quote=False)
    s=re.sub(r"\*\*(.+?)\*\*",r"<b>\1</b>",s)
    s=re.sub(r"\*(.+?)\*",r"<i>\1</i>",s)
    s=re.sub(r"\x60([^\x60]+)\x60",r'<font name="Mono" size="8">\1</font>',s)
    s=re.sub(r"\[([^\]]+)\]\((https?://[^)]+)\)",r'<link href="\2" color="#28566b">\1</link>',s)
    return s
class Diagram(Flowable):
    def __init__(self,kind,lang):
        Flowable.__init__(self);self.kind=kind;self.lang=lang;self.width=451
        self.height=160 if kind=="architecture" else 110
    def draw(self):
        c=self.canv;zh=self.lang=="zh";font="CJK" if zh else "Sans"
        def label(x,y,s,size=8):
            c.setFillColor(INK);c.setFont(font,size);c.drawCentredString(x,y,s)
        def box(x,y,w,h,s):
            c.setFillColor(PALE);c.setStrokeColor(ACCENT);c.roundRect(x,y,w,h,3,fill=1,stroke=1)
            label(x+w/2,y+h/2-3,s)
        def arrow(x,y,xx,yy):
            c.setStrokeColor(ACCENT);c.line(x,y,xx,yy)
            if xx>x:c.line(xx,yy,xx-4,yy+2);c.line(xx,yy,xx-4,yy-2)
            elif xx<x:c.line(xx,yy,xx+4,yy+2);c.line(xx,yy,xx+4,yy-2)
            else:c.line(xx,yy,xx-2,yy+4);c.line(xx,yy,xx+2,yy+4)
        if self.kind=="architecture":
            a=["设计与策略","带来源的图","有界上下文","受控修改","版本绑定验证","偏离与协调"] if zh else [
                "Design + policy","Provenanced graph","Bounded context","Guarded mutation","Bound verification","Drift + reconciliation"]
            positions=[(0,100),(153,100),(306,100),(306,43),(153,43),(0,43)]
            for (x,y),s in zip(positions,a):box(x,y,145,31,s)
            arrow(145,115,153,115);arrow(298,115,306,115);arrow(378,100,378,74)
            arrow(306,58,298,58);arrow(153,58,145,58)
            box(0,0,451,25,"观测：源码、关系、任务、风险、证据、权限和运行状态" if zh else
                "Observability: source, relations, work, risk, evidence, access, runtime")
            label(225,146,"模型可替换；工程状态和证据由运行时保留" if zh else
                "Replaceable model; engineering state and evidence remain in the runtime")
        else:
            a=["被引用","已执行","通过","仍新鲜","可用于验收"] if zh else ["Referenced","Executed","Passed","Fresh","Admissible"]
            for i,s in enumerate(a):
                box(i*91,57,83,28,s)
                if i<4:arrow(i*91+83,71,(i+1)*91,71)
            label(225,34,"每一步都有独立条件，不能直接从前一步推断" if zh else
                "Every transition has separate conditions; preceding states do not imply later states.")
            label(225,15,"版本变化会使证据过期；其他来源的新失败必须保留" if zh else
                "Revision changes stale evidence; new failures from other producers remain visible.")
def make_styles(lang):
    ss=getSampleStyleSheet();zh=lang=="zh";font="CJK" if zh else "Roman"
    ss.add(ParagraphStyle("BodyP",fontName=font,fontSize=10.2 if zh else 11,
        leading=16 if zh else 14.5,spaceAfter=7,alignment=TA_JUSTIFY,
        wordWrap="CJK" if zh else None,splitLongWords=True))
    ss.add(ParagraphStyle("TitleP",fontName="CJK" if zh else "Bold",fontSize=18,leading=23,spaceAfter=12))
    ss.add(ParagraphStyle("MetaP",fontName=font,fontSize=8.2,leading=11,textColor=GRAY,spaceAfter=10))
    for level,size in [(2,12),(3,10.7),(4,10.5)]:
        ss.add(ParagraphStyle(f"H{level}",fontName="CJK" if zh else "Bold",fontSize=size,leading=size+4,
            spaceBefore=12,spaceAfter=6,keepWithNext=True))
    for name,size,lead in [("CellP",8.2,11),("CaptionP",8.8,12),("RefP",9,12)]:
        ss.add(ParagraphStyle(name,fontName=font,fontSize=size,leading=lead,spaceAfter=5,
            wordWrap="CJK" if zh else None,splitLongWords=True))
    ss.add(ParagraphStyle("CodeP",fontName="Mono",fontSize=7.5,leading=10,spaceAfter=8,backColor=PALE))
    return ss
def make_table(lines,ss):
    cells=[[x.strip() for x in l.strip("|").split("|")] for l in lines]
    cells=[r for r in cells if not all(re.fullmatch(r"[: -]+",x) for x in r)]
    n=len(cells[0])
    if n==2:widths=[143,308]
    elif n==3:widths=[123,164,164]
    elif n==4:widths=[115,112,112,112]
    elif n==5:widths=[83,92,92,92,92]
    else:widths=[451/n]*n
    tb=Table([[Paragraph(inline(x),ss["CellP"]) for x in r] for r in cells],colWidths=widths,repeatRows=1,hAlign="LEFT")
    tb.setStyle(TableStyle([("BACKGROUND",(0,0),(-1,0),PALE),("LINEABOVE",(0,0),(-1,0),.7,ACCENT),
        ("LINEBELOW",(0,0),(-1,0),.45,ACCENT),("LINEBELOW",(0,-1),(-1,-1),.5,ACCENT),
        ("VALIGN",(0,0),(-1,-1),"TOP"),("LEFTPADDING",(0,0),(-1,-1),5),("RIGHTPADDING",(0,0),(-1,-1),5),
        ("TOPPADDING",(0,0),(-1,-1),5),("BOTTOMPADDING",(0,0),(-1,-1),5)]))
    return tb
def render(path):
    lang="zh" if ".zh-CN." in path.name else "en";ss=make_styles(lang)
    text=path.read_text();lines=text.splitlines();flow=[];i=0;refs=False
    journal=path.name.split(".")[0];name={"softwarex":"SoftwareX","spe":"Software: Practice and Experience","jss":"Journal of Systems and Software"}[journal]
    while i<len(lines):
        line=lines[i].strip()
        if not line:i+=1;continue
        if line.startswith("@figure "):
            flow.append(KeepTogether([Diagram(line.split()[1],lang),Paragraph(inline(lines[i+1].strip()),ss["CaptionP"])]));i+=2;continue
        if line=="@pagebreak":flow.append(PageBreak());i+=1;continue
        if line.startswith("|"):
            block=[]
            while i<len(lines) and lines[i].strip().startswith("|"):block.append(lines[i].strip());i+=1
            flow.extend([make_table(block,ss),Spacer(1,9)]);continue
        if line.startswith("~~~") or line.startswith("\x60\x60\x60"):
            fence=line[:3];block=[];i+=1
            while i<len(lines) and not lines[i].startswith(fence):block.append(lines[i]);i+=1
            flow.append(Paragraph("<br/>".join(html.escape(x).replace(" ","&#160;") for x in block),ss["CodeP"]));i+=1;continue
        m=re.match(r"^(#{1,4}) (.+)$",line)
        if m:
            lvl=len(m[1]);title=m[2];refs=refs or "References" in title or "参考文献" in title
            flow.append(Paragraph(inline(title),ss["TitleP" if lvl==1 else f"H{lvl}"]));i+=1;continue
        if line.startswith("@meta "):
            flow.append(Paragraph(inline(line[6:]),ss["MetaP"]));i+=1;continue
        block=[line];i+=1
        while i<len(lines) and lines[i].strip() and not re.match(r"^(#|\||@|~~~|\x60\x60\x60|- )",lines[i].strip()):
            block.append(lines[i].strip());i+=1
        txt=("".join(block) if lang=="zh" else " ".join(block))
        if txt.startswith("- "):txt="• "+txt[2:]
        flow.append(Paragraph(inline(txt),ss["RefP" if refs else "BodyP"]))
    target=OUT/(path.stem+".pdf")
    def page(c,doc):
        c.saveState();c.setFont("Sans",7);c.setFillColor(GRAY)
        c.drawString(72,811,"wcode | "+name+" | Review manuscript")
        c.setStrokeColor(colors.HexColor("#ccd5da"));c.setLineWidth(.4);c.line(72,801,523,801)
        c.drawString(72,28,"6 November 2026 | "+("Chinese companion" if lang=="zh" else "English manuscript"))
        c.drawRightString(523,28,str(doc.page));c.restoreState()
    doc=SimpleDocTemplate(str(target),pagesize=(595.28,841.89),leftMargin=72,rightMargin=72,
        topMargin=52,bottomMargin=50,title=lines[0].removeprefix("# "),author="",
        subject="wcode research manuscript; venue-targeted alternative",pageCompression=1)
    doc.build(flow,onFirstPage=page,onLaterPages=page)
    reader=PdfReader(target);extracted="\n".join(p.extract_text() or "" for p in reader.pages)
    if len(extracted)<3000 or "\ufffd" in extracted:raise ValueError("PDF text validation failed: "+str(target))
    return {"pdf":str(target.relative_to(HERE)),"source":path.name,"pages":len(reader.pages),
        "source_sha256":hashlib.sha256(path.read_bytes()).hexdigest(),"pdf_sha256":hashlib.sha256(target.read_bytes()).hexdigest(),
        "bytes":target.stat().st_size,"extracted_chars":len(extracted),"word_count":len(re.findall(r"\b[\w'-]+\b",text)) if lang=="en" else None}
def main():
    OUT.mkdir(parents=True,exist_ok=True)
    sources=sorted(HERE.glob("*.en.md"))+sorted(HERE.glob("*.zh-CN.md"))
    if len(sources)!=6:raise ValueError(f"Expected six sources, found {len(sources)}")
    results=[render(p) for p in sources]
    (HERE/"build-manifest.json").write_text(json.dumps({"builder":"ReportLab; embedded fonts","font_files":FONTS,"artifacts":results},ensure_ascii=False,indent=2)+"\n")
    print(json.dumps(results,ensure_ascii=False,indent=2))
if __name__=="__main__":main()
