// Room annotations share the view's canvas and print/export path.
const Spatial = (() => {
  const sq=(p,r)=>[[p.x-r,p.y-r],[p.x+r,p.y-r],[p.x+r,p.y+r],[p.x-r,p.y+r]];
  function elements(data,k){
    const out=[];
    // Pick a room at its location cross, so rooms do not cover the elements inside them.
    for(const r of data.rooms||[])out.push({id:r.id,kind:'room',category:'Rooms',ring:sq(r.at,4/k),pinned:r.pinned,display:'normal'});
    for(const r of data.room_tags||[])out.push({id:r.id,kind:'roomtag',category:'Room Tags',ring:sq(r.at,9*(data.view?.scale||100)),pinned:r.pinned,display:'normal'});
    for(const r of data.room_separators||[])for(let i=1;i<r.pts.length;i++){const a=r.pts[i-1],b=r.pts[i],l=Math.hypot(b[0]-a[0],b[1]-a[1])||1,nx=-(b[1]-a[1])*3/k/l,ny=(b[0]-a[0])*3/k/l;out.push({id:r.id,kind:'roomseparator',category:'Room Separation',ring:[[a[0]+nx,a[1]+ny],[b[0]+nx,b[1]+ny],[b[0]-nx,b[1]-ny],[a[0]-nx,a[1]-ny]],pinned:r.pinned,display:'normal'});}
    return out;
  }
  function draw(ctx,data,toScreen,k,sel,printing){
    ctx.save();
    const path=(pts,close)=>{ctx.beginPath();pts.forEach((p,i)=>{const q=toScreen({x:p[0],y:p[1]});i?ctx.lineTo(q.x,q.y):ctx.moveTo(q.x,q.y);});if(close)ctx.closePath();};
    for(const s of data.room_separators||[]){ctx.strokeStyle=sel.has(s.id)?'#2e6f9e':'#65866c';ctx.lineWidth=sel.has(s.id)?2:1;path(s.pts,false);ctx.stroke();}
    if(!printing)for(const r of data.rooms||[]){const p=toScreen(r.at);if(sel.has(r.id)){ctx.fillStyle='rgba(46,111,158,0.16)';ctx.beginPath();for(const ring of r.rings){ring.forEach((p,i)=>{const q=toScreen({x:p[0],y:p[1]});i?ctx.lineTo(q.x,q.y):ctx.moveTo(q.x,q.y);});ctx.closePath();}ctx.fill('evenodd');}ctx.strokeStyle=r.status==='placed'?'#65866c':'#b63727';ctx.beginPath();ctx.moveTo(p.x-4,p.y-4);ctx.lineTo(p.x+4,p.y+4);ctx.moveTo(p.x-4,p.y+4);ctx.lineTo(p.x+4,p.y-4);ctx.stroke();}
    for(const t of data.room_tags||[]){const p=toScreen(t.at),b={x:p.x,y:p.y,k:k*(data.view?.scale||100)};const col=sel.has(t.id)?'#2e6f9e':null;if(t.leader&&t.room_at){const q=toScreen(t.room_at);ctx.strokeStyle=col||'#333';ctx.beginPath();ctx.moveTo(p.x,p.y);ctx.lineTo(q.x,q.y);ctx.stroke();}if(t.family)Ann.paths(ctx,t.family.body,b,col,'#fff');Ann.labels(ctx,t.labels||[],b,col);}
    ctx.restore();
  }
  // Room and Tag Room run in the plan like any placing tool: their choices are panels on the Modify tab (Tag on
  // Placement, Place Rooms Automatically, a new room or one of the unplaced rooms), not a dialog
  let unplaced=[];
  async function start(tag=false){if(!(await ensurePlan(tag?'A room tag':'A room')))return;
    if(tag){Plan.setTool('roomtag');updateRibbonContext(true);setPrompt('Click inside a placed room to tag it. Esc finishes.');return;}
    unplaced=(await Backend.call('rooms',{})).filter(r=>!r.at);
    const o=app.roomPlacement||{};app.roomPlacement={id:unplaced.some(r=>r.id===o.id)?o.id:null,tag:o.tag!==false};
    Plan.setTool('room');updateRibbonContext(true);
  }
  function context(){
    const mode={label:'Mode',items:[{label:'Modify',ico:'Se',big:true,onclick:()=>Plan.setTool('select')}]};
    if(Plan.tool==='roomtag')return{auto:true,key:'roomtag',title:'Tag Room',groups:[mode]};
    const o=app.roomPlacement||(app.roomPlacement={id:null,tag:true});
    const pick=el('select',{title:'Room: a new room, or an unplaced room with its data'},[new Option('New',''),...unplaced.map(r=>new Option(`${r.number} ${r.name}`,r.id,false,r.id===o.id))]);
    pick.onchange=()=>{o.id=Number(pick.value)||null;};
    return{auto:true,key:`room:${o.tag}:${unplaced.length}`,title:'Place Room',groups:[mode,
      {label:'Room',items:[{label:'Tag on Placement',icon:'tagRoom',on:o.tag,onclick:()=>{o.tag=!o.tag;updateRibbonContext();}},{label:'Place Rooms Automatically',ico:'Au',onclick:auto},{label:'Area and Volume Computations',ico:'AV',onclick:settings}]},
      {label:'Room to Place',items:[{node:pick}]}]};
  }
  async function auto(){const o=app.roomPlacement||{tag:true};const r=await run('place_all_rooms',{level_id:curLevelId(),view_id:app.view.viewId,tag:o.tag});if(!r)return;await refresh();setPrompt(r.ids.length?`Placed ${r.ids.length} room${r.ids.length===1?'':'s'}.`:'Every enclosed area on this level already has a room.');}
  async function place(tool,p){
    if(tool==='roomtag'){const r=await run('room_at',{level_id:curLevelId(),point:[p.x,p.y]});if(r?.id){await run('add_room_tag',{view_id:app.view.viewId,room_id:r.id,point:[p.x,p.y]});await refresh();}else setPrompt('Click inside an enclosed, placed room.',true);return;}
    const o=app.roomPlacement||{tag:true};const r=await run(o.id?'place_room':'add_room',{id:o.id,level_id:curLevelId(),view_id:app.view.viewId,point:[p.x,p.y],tag:o.tag});if(!r)return;
    if(o.id){if(o.tag)await run('add_room_tag',{view_id:app.view.viewId,room_id:o.id,point:[p.x,p.y]});unplaced=unplaced.filter(x=>x.id!==o.id);o.id=null;}
    await refresh();updateRibbonContext();
  }
  async function separators(){if(Sketch.active||!(await ensurePlan('Room separation lines')))return;Plan.startSketch({kind:'roomseparation',title:'Room Separation Lines',levelId:curLevelId(),curves:[],tool:'line',closed:false});renderSketchUI();}
  async function finish(o){const r=await run('set_room_separators',{level_id:o.levelId,curves:o.curves});if(!r)return false;Plan.endSketch();endSketchUI();await refresh();return true;}
  async function settings(){const s=await Backend.call('room_settings',{});openModal('Area and Volume Computations',(body,foot,close)=>{const boundary=el('select',{},[['finish','At wall finish'],['centre','At wall centre'],['core_face','At core face'],['core_centre','At core centre']].map(([v,l])=>new Option(l,v,false,s.boundary===v)));const volumes=el('input',{type:'checkbox',checked:s.volumes});body.append(el('label',{className:'new-row'},['Room boundary',boundary]),el('label',{},[volumes,' Calculate room volumes']));foot.append(button('Area Schemes',()=>{close();ColorFill.schemes();}),button('Cancel',close),button('OK',async()=>{if(await run('set_room_settings',{settings:{boundary:boundary.value,volumes:volumes.checked}})){close();await refresh();}},true));});}
  async function insertRoom(){openModal("New Unplaced Room",(body,foot,close)=>{const level=el("select",{},app.info.levels.map(l=>new Option(l.name,l.id)));body.append(el("label",{className:"new-row"},["Level",level]));foot.append(button("Cancel",close),button("Add Room",async()=>{if(await run("add_room",{level_id:Number(level.value),tag:false})){close();await refresh();}},true));});}
  return {elements,draw,start,context,place,separators,finish,settings,insertRoom};
})();
