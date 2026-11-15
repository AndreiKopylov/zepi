use ab_glyph::{Font, FontVec};
use gridcraft_core::{CellRef, Value};
use gridcraft_model::style::{Color, HAlign, VAlign};

const PT: f32 = 4.0 / 3.0;

struct Fonts { reg: FontVec, bold: FontVec, ital: FontVec, boldital: FontVec }
impl Fonts {
    fn load() -> Self {
        let rd = |p: &str| FontVec::try_from_vec(std::fs::read(p).unwrap()).unwrap();
        Fonts { reg: rd("C:/Windows/Fonts/times.ttf"), bold: rd("C:/Windows/Fonts/timesbd.ttf"), ital: rd("C:/Windows/Fonts/timesi.ttf"), boldital: rd("C:/Windows/Fonts/timesbi.ttf") }
    }
    fn get(&self, b: bool, i: bool) -> &FontVec { match (b, i) { (true, true) => &self.boldital, (true, false) => &self.bold, (false, true) => &self.ital, _ => &self.reg } }
}
fn scale(f: &FontVec, px: f32) -> f32 { px / f.units_per_em().unwrap() }
fn metrics(f: &FontVec, px: f32) -> (f32, f32) { let s = scale(f, px); (f.ascent_unscaled()*s, -f.descent_unscaled()*s) }
fn width(f: &FontVec, px: f32, t: &str) -> f32 {
    let s = scale(f, px); let mut w = 0.0; let mut prev = None;
    for ch in t.chars() { let g = f.glyph_id(ch); if let Some(p)=prev { w += (f.kern_unscaled(p,g)*s).round(); } w += (f.h_advance_unscaled(g)*s).round(); prev = Some(g); }
    w
}
fn draw(img: &mut image::RgbImage, f: &FontVec, px: f32, t: &str, x: f32, base: f32, col: [u8;3]) {
    let s = scale(f, px); let mut pen = x; let mut prev = None;
    for ch in t.chars() {
        let gid = f.glyph_id(ch);
        if let Some(p)=prev { pen += f.kern_unscaled(p,gid)*s; }
        if let Some(o) = f.outline_glyph(gid.with_scale(px)) {
            let b = o.bounds();
            o.draw(|gx,gy,cov| {
                let X=(pen+b.min.x+gx as f32) as i32; let Y=(base+b.min.y+gy as f32) as i32;
                if X>=0&&Y>=0&&(X as u32)<img.width()&&(Y as u32)<img.height() {
                    let p=img.get_pixel_mut(X as u32,Y as u32);
                    for k in 0..3 { p[k]=(p[k] as f32*(1.0-cov)+col[k] as f32*cov) as u8; }
                }
            });
        }
        pen += f.h_advance_unscaled(gid)*s; prev = Some(gid);
    }
}
fn tokens(seg: &str) -> Vec<String> {
    let mut out = Vec::new();
    for w in seg.split_inclusive(' ') { for t in w.split_inclusive('-') { if !t.is_empty() { out.push(t.to_string()); } } }
    out
}
fn rgb(c: Option<u32>) -> [u8;3] { c.map(|v| [(v>>16) as u8,(v>>8) as u8,v as u8]).unwrap_or([0,0,0]) }
fn col(c: Color, th: &[u32;12]) -> [u8;3] { match c { Color::Rgb(v)=>[(v>>16) as u8,(v>>8) as u8,v as u8], Color::Theme(i,_)=>{ let v=th[(i as usize).min(11)]; [(v>>16) as u8,(v>>8) as u8,v as u8] }, _=>[0,0,0] } }

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let out = std::env::args().nth(2).unwrap_or_else(|| "C:/temp/probe.png".into());
    let bytes = std::fs::read(&path).unwrap();
    let (wb, _) = gridcraft_xlsx::read_xlsx(&bytes).unwrap();
    let sheet = &wb.sheets[0];
    let fonts = Fonts::load();
    let ur = sheet.used_range().unwrap();
    let maxr = 11u32.min(ur.end.row);
    let maxc = 18u32.min(ur.end.col);
    let xs: Vec<f32> = (0..=maxc+1).map(|c| sheet.col_left(c) as f32).collect();
    let ys: Vec<f32> = (0..=maxr+1).map(|r| sheet.row_top(r) as f32).collect();
    let x0g = xs[0]; let y0g = ys[0];
    let W = (xs[(maxc+1) as usize]-x0g) as u32+2; let H = (ys[(maxr+1) as usize]-y0g) as u32+2;
    let mut img = image::RgbImage::from_pixel(W, H, image::Rgb([255,255,255]));
    let X = |c: u32| (xs[c as usize]-x0g); let Y = |r: u32| (ys[r as usize]-y0g);
    for c in 0..=maxc+1 { let xx=X(c) as u32; for yy in 0..H { if xx<W { img.put_pixel(xx,yy,image::Rgb([215,215,215])); } } }
    for r in 0..=maxr+1 { let yy=Y(r) as u32; for xx in 0..W { if yy<H { img.put_pixel(xx,yy,image::Rgb([215,215,215])); } } }
    let mut done = std::collections::HashSet::new();
    for r in 0..=maxr { for c in 0..=maxc {
        let at = CellRef::new(r,c);
        let (r0,c0,r1,c1) = match sheet.merge_at(at) { Some(m)=>(m.start.row,m.start.col,m.end.row,m.end.col), None=>(r,c,r,c) };
        if !done.insert((r0,c0)) { continue; }
        let cell = match sheet.cell(CellRef::new(r0,c0)) { Some(c)=>c, None=>continue };
        let text = match &cell.value { Value::Text(t)=>t.to_string(), Value::Number(n)=>format!("{}",n), _=>continue };
        if text.is_empty() { continue; }
        let st = wb.styles.get(sheet.style_id(CellRef::new(r0,c0)));
        let cx0=X(c0); let cx1=X(c1+1); let cy0=Y(r0); let cy1=Y(r1+1);
        let cw=cx1-cx0; let chh=cy1-cy0;
        let b=&st.borders; let blk=image::Rgb([0,0,0]);
        if !b.left.is_none() { for yy in cy0 as u32..cy1 as u32 { img.put_pixel(cx0 as u32,yy,blk); } }
        if !b.right.is_none() { for yy in cy0 as u32..cy1 as u32 { img.put_pixel((cx1-1.0) as u32,yy,blk); } }
        if !b.top.is_none() { for xx in cx0 as u32..cx1 as u32 { img.put_pixel(xx,cy0 as u32,blk); } }
        if !b.bottom.is_none() { for xx in cx0 as u32..cx1 as u32 { img.put_pixel(xx,(cy1-1.0) as u32,blk); } }
        let mut runs: Vec<(String,f32,[u8;3],bool,bool)> = Vec::new();
        if let Some(rs) = wb.rich.iter().find(|r| r.text == text) {
            for run in &rs.runs { runs.push((run.text.clone(), run.size.unwrap_or(st.font.size)*PT, rgb(run.color), run.bold||st.font.bold, run.italic||st.font.italic)); }
        } else {
            runs.push((text.clone(), st.font.size*PT, col(st.font.color, &wb.theme.colors), st.font.bold, st.font.italic));
        }
        let inner_w = cw+2.0;
        let base_h = { let (a,d)=metrics(fonts.get(st.font.bold,st.font.italic), st.font.size*PT); a+d };
        let mut lines: Vec<Vec<(String,f32,[u8;3],bool,bool,f32,f32)>> = vec![Vec::new()];
        let mut curw=0.0;
        for (t,sz,col,bo,it) in &runs {
            let f=fonts.get(*bo,*it); let (asc,desc)=metrics(f,*sz);
            let mut first=true;
            for seg in t.split('\n') {
                if !first { lines.push(Vec::new()); curw=0.0; } first=false;
                for word in tokens(seg) {
                    if word.is_empty(){continue;}
                    let w=width(f,*sz,&word);
                    if st.align.wrap && curw+w>inner_w && curw>0.0 { lines.push(Vec::new()); curw=0.0; }
                    lines.last_mut().unwrap().push((word.to_string(),*sz,*col,*bo,*it,w,asc+desc));
                    curw+=w;
                }
            }
        }
        let lhs: Vec<f32> = lines.iter().map(|l| if l.is_empty(){base_h} else {l.iter().map(|w| w.6).fold(0.0f32,f32::max)}).collect();
        let total: f32 = lhs.iter().sum();
        let mut yy = match st.align.v { VAlign::Center=>cy0+((chh-total)/2.0).max(0.0), VAlign::Bottom=>cy0+(chh-total).max(0.0), _=>cy0 };
        for (li,line) in lines.iter().enumerate() {
            let lh=lhs[li];
            let lw: f32 = line.iter().map(|w| w.5).sum();
            let mut xx = match st.align.h { HAlign::Center|HAlign::CenterAcross=>cx0+2.0+((inner_w-lw)/2.0).max(0.0), HAlign::Right=>cx0+cw-2.0-lw, _=>cx0+2.0 };
            for (word,sz,col,bo,it,w,_h) in line {
                let f=fonts.get(*bo,*it); let (asc,_)=metrics(f,*sz);
                draw(&mut img,f,*sz,word,xx,yy+asc,*col); xx+=w;
            }
            yy+=lh;
        }
    }}
    img.save(&out).unwrap();
    println!("saved {} {}x{}", out, W, H);
}
