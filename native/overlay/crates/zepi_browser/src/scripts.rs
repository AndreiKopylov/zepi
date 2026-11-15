// Page-side helpers (adapted from browser-use, MIT): resolve an indexed node to a
// safe click point and settle autocomplete after input. Injected via Runtime.evaluate.

// ARIA-tree DOM extraction vendored from BB-fat/browser-use-rs (MIT).
pub const DOM_JS: &str = include_str!("dom.js");

pub const TARGET: &str = r#"(action => {
  let e=(window.__zepiResolve?window.__zepiResolve(action.node):window.__zepiBrowser?.nodes.get(action.node));
  if (!e) return {why:'no-node'};
  if (!e.isConnected) return {why:'disconnected'};
  if (e.matches(':disabled') || e.closest('[aria-disabled="true"],[inert]')) return {why:'disabled'};
  if (!e.checkVisibility({checkOpacity:true,checkVisibilityCSS:true})) return {why:'hidden'};
  if (action.kind==='fill' && (e.readOnly || e.getAttribute('aria-readonly')==='true')) return {why:'readonly'};
  let r=e.getBoundingClientRect();
  const cy=r.y+r.height/2;
  if (r.bottom<0 || r.top>innerHeight || cy<0 || cy>=innerHeight) { e.scrollIntoView({block:'center'}); e=(window.__zepiResolve?window.__zepiResolve(action.node):e)||e; r=e.getBoundingClientRect(); }
  const x=r.x+r.width/2, y=r.y+r.height/2;
  if (!r.width || !r.height) return {why:'zero-size'};
  if (x<0 || y<0 || x>=innerWidth || y>=innerHeight) return {why:'offscreen'};
  if (!e.contains(document.elementFromPoint(x,y))) return {why:'occluded'};
  if (action.kind==='select') {
    if (e.tagName!=='SELECT' || ![...e.options].some(o=>o.value===action.value &&
        !o.disabled && !o.closest('optgroup[disabled]'))) return null;
    e.value=action.value;
    e.dispatchEvent(new Event('input',{bubbles:true}));
    e.dispatchEvent(new Event('change',{bubbles:true}));
  }
  return {x,y};
})("#;

pub const AFTER_INPUT: &str = r#"(action => new Promise(resolve => {
  const field=window.__zepiResolve ? window.__zepiResolve(action.node) : window.__zepiBrowser?.nodes.get(action.node);
  const autocomplete=(action.kind==='fill'||action.kind==='click') && field?.getAttribute('role')==='combobox';
  let frames=0, stopped=false;
  const finish=()=>{stopped=true;resolve()};
  setTimeout(finish,autocomplete ? 500 : 50);
  const ready=()=>{
    if (stopped) return;
    const ids=(field?.getAttribute('aria-controls')||field?.getAttribute('aria-owns')||'')
      .split(/\s+/).filter(Boolean);
    const roots=ids.length ? ids.map(id=>document.getElementById(id)).filter(Boolean) : [document];
    const options=roots.flatMap(root=>[...root.querySelectorAll('[role="option"]')]);
    if (++frames>=2 && (!autocomplete || options.some(e=>{
      const r=e.getBoundingClientRect();
      return r.width && r.height && r.bottom>0 && r.top<innerHeight &&
        e.checkVisibility({checkOpacity:true,checkVisibilityCSS:true});
    }))) finish();
    else requestAnimationFrame(ready);
  };
  requestAnimationFrame(ready);
}))("#;