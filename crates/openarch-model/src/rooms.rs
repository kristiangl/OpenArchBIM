//! Live room enclosures, room separation lines and room-aware component membership.
use std::{collections::BTreeMap, sync::Arc};
use serde::{Deserialize, Serialize};
use crate::{Document, Element, Id, Point};
use crate::geom::Region;
use crate::curve::{self, Curve};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all="snake_case")]
pub enum RoomBoundary { #[default] Finish, Centre, CoreFace, CoreCentre }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomSettings {
    #[serde(default)] pub boundary: RoomBoundary,
    #[serde(default)] pub volumes: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Room {
    pub level: Id,
    #[serde(default)] pub at: Option<Point>,
    pub number: String,
    pub name: String,
    #[serde(default)] pub upper_level: Option<Id>,
    #[serde(default="default_height")] pub limit_offset_mm: f64,
    #[serde(default)] pub base_offset_mm: f64,
    #[serde(default)] pub comments: String,
    #[serde(default)] pub occupancy: String,
    #[serde(default)] pub department: String,
    #[serde(default)] pub occupant: String,
    #[serde(default)] pub base_finish: String,
    #[serde(default)] pub floor_finish: String,
    #[serde(default)] pub wall_finish: String,
    #[serde(default)] pub ceiling_finish: String,
    #[serde(default)] pub phase: Option<Id>,
    #[serde(default)] pub pinned: bool,
    #[serde(default)] pub ifc: crate::element::IfcParams,
}
fn default_height()->f64 {4000.0}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomSeparation { pub level: Id, pub curve: Curve, #[serde(default)] pub phase: Option<Id>, #[serde(default)] pub pinned: bool }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoomTag { pub view: Id, pub room: Id, pub at: Point, #[serde(default)] pub family: Option<Id>, #[serde(default)] pub leader: bool, #[serde(default)] pub pinned: bool }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all="snake_case")]
pub enum RoomStatus { Placed, NotPlaced, NotEnclosed, Redundant }

#[derive(Debug, Clone)]
pub struct RoomCalculation { pub status: RoomStatus, pub region: Option<Region>, pub area_mm2: f64, pub perimeter_mm: f64, pub volume_mm3: Option<f64>, pub height_mm: f64 }

#[derive(Debug, Clone, Default)]
pub(crate) struct RoomCache { pub edit: u64, pub faces: BTreeMap<Id,Arc<Vec<Region>>>, pub calculations: BTreeMap<Id,RoomCalculation>, pub caps: Option<Arc<Vec<[[f64;3];3]>>> }

pub fn contains(region:&Region,at:Point)->bool {curve::point_in_polygon(at,&region.outer)&&!region.holes.iter().any(|h|curve::point_in_polygon(at,h))}

fn strip(a:Point,b:Point,left:f64,right:f64,extend:f64)->Vec<Point>{let l=a.dist(&b);if l<1e-6{return vec![];}let (dx,dy)=((b.x-a.x)/l,(b.y-a.y)/l);vec![Point::new(a.x-dx*extend+dy*right,a.y-dy*extend-dx*right),Point::new(b.x+dx*extend+dy*right,b.y+dy*extend-dx*right),Point::new(b.x+dx*extend-dy*left,b.y+dy*extend+dx*left),Point::new(a.x-dx*extend-dy*left,a.y-dy*extend+dx*left)]}

impl Document {
    fn room_cache_reset(&self){let mut c=self.room_cache.borrow_mut();if c.edit!=self.edit_count{*c=RoomCache{edit:self.edit_count,..Default::default()};}}
    pub fn rooms(&self)->Vec<(Id,&Room)>{self.iter().filter_map(|(id,e)|if let Element::Room(r)=e{Some((id,r))}else{None}).collect()}
    pub fn room(&self,id:Id)->Option<&Room>{match self.get(id){Some(Element::Room(r))=>Some(r),_=>None}}
    pub fn room_tags(&self,view:Id)->Vec<(Id,&RoomTag)>{self.iter().filter_map(|(id,e)|match e{Element::RoomTag(t)if t.view==view=>Some((id,t)),_=>None}).collect()}
    pub fn room_settings(&self)->RoomSettings{self.iter().find_map(|(_,e)|if let Element::RoomSettings(s)=e{Some(s.clone())}else{None}).unwrap_or_default()}
    pub fn set_room_settings(&mut self,s:RoomSettings)->Result<(),String>{let id=self.iter().find_map(|(id,e)|matches!(e,Element::RoomSettings(_)).then_some(id));self.transact("Area and volume computations",|tx|match id{Some(id)=>tx.update(id,Element::RoomSettings(s)),None=>{tx.add(Element::RoomSettings(s));Ok(())}})}
    pub fn room_height(&self,r:&Room)->f64{let base=self.level(r.level).map_or(0.0,|l|l.elevation_mm);let top=r.upper_level.and_then(|id|self.level(id)).map_or(base,|l|l.elevation_mm)+r.limit_offset_mm;top-base-r.base_offset_mm}

    /// Enclosed free-space faces after subtracting room-bounding wall bodies and separation lines.
    pub fn room_faces(&self,level:Id)->Arc<Vec<Region>>{
        self.room_cache_reset();if let Some(f)=self.room_cache.borrow().faces.get(&level){return f.clone();}
        let z=self.level(level).map_or(0.0,|l|l.elevation_mm+l.computation_height_mm);
        let faces=Arc::new(self.enclosed_faces(level,z,self.room_settings().boundary,true));self.room_cache.borrow_mut().faces.insert(level,faces.clone());faces
    }

    /// The free-space faces walls (that reach height `z`), room-bounding columns and families, and (with `separators`)
    /// room separation lines enclose on a level: what rooms and automatic ceilings fill.
    pub fn enclosed_faces(&self,level:Id,z:f64,boundary:RoomBoundary,separators:bool)->Vec<Region>{
        let settings=RoomSettings{boundary,..Default::default()};
        let mut walls=vec![];
        for (id,w) in self.iter().filter_map(|(id,e)|if let Element::Wall(w)=e{Some((id,w))}else{None}){
            let (lo,hi)=self.wall_extent(w);if !w.room_bounding||z<lo-0.01||z>=hi-0.001{continue;}
            let mut facets=self.wall_facets(id,w);
            if settings.boundary!=RoomBoundary::Finish{
                let (left,right)=self.wall_extents(w);let mut before=0.0;let mut after=0.0;
                if let Some(t)=self.wall_type(w.wall_type){let first=t.layers.iter().position(|l|l.core);let last=t.layers.iter().rposition(|l|l.core);if let(Some(first),Some(last))=(first,last){before=t.layers[..first].iter().map(|l|l.thickness_mm).sum();after=t.layers[last+1..].iter().map(|l|l.thickness_mm).sum();}}
                let (cl,cr)=if w.flipped{(left-after,right-before)}else{(left-before,right-after)};
                // the middle of the core, or of the whole wall body: not the location line, which can be a face
                let centre=if settings.boundary==RoomBoundary::CoreCentre{(cl-cr)/2.0}else{(left-right)/2.0};
                for f in &mut facets{if settings.boundary==RoomBoundary::CoreFace{f.left=cl;f.right=cr;}else{f.left=centre+0.005;f.right=-centre+0.005;}}
            }
            for f in &mut facets{f.level=level;}walls.extend(facets);
        }
        let mut clips:Vec<Vec<Point>>=crate::join::join_walls(&walls).into_values().map(|q|q.to_vec()).collect();
        for (_,e) in self.iter().filter(|_|separators){if let Element::RoomSeparation(s)=e{if s.level==level{let pts=s.curve.tessellate(1.5);for p in pts.windows(2){clips.push(strip(p[0],p[1],0.005,0.005,0.02));}}}}
        // Columns and explicitly room-bounding loadable families contribute their actual horizontal mesh section.
        for (id,i) in self.instances(){let Some((family,_))=self.instance_family(i)else{continue};let bounded=family.settings.room_bounding.unwrap_or(matches!(family.category,crate::family::FamilyCategory::Columns|crate::family::FamilyCategory::StructuralColumns));if !bounded{continue;}if let Some((_,solid,_))=crate::mesh::instance_meshes(self,id){clips.extend(horizontal_rings(&solid,z+0.001));}}
        let pts:Vec<_>=clips.iter().flatten().copied().collect();let faces=if pts.is_empty(){vec![]}else{
            let minx=pts.iter().map(|p|p.x).fold(f64::INFINITY,f64::min);let maxx=pts.iter().map(|p|p.x).fold(f64::NEG_INFINITY,f64::max);let miny=pts.iter().map(|p|p.y).fold(f64::INFINITY,f64::min);let maxy=pts.iter().map(|p|p.y).fold(f64::NEG_INFINITY,f64::max);let pad=(maxx-minx).max(maxy-miny).max(1000.0);
            let frame=vec![Point::new(minx-pad,miny-pad),Point::new(maxx+pad,miny-pad),Point::new(maxx+pad,maxy+pad),Point::new(minx-pad,maxy+pad)];
            crate::layers::subtract(&frame,clips.iter()).into_iter().filter_map(|mut s|{if s.is_empty(){return None;}let outer=s.remove(0);if outer.iter().any(|p|p.x<=minx-pad+0.1||p.x>=maxx+pad-0.1||p.y<=miny-pad+0.1||p.y>=maxy+pad-0.1){return None;}let r=Region{outer,holes:s};(r.area()>100.0).then_some(r)}).collect()
        };
        faces
    }

    pub fn room_calculation(&self,id:Id)->Option<RoomCalculation>{
        self.room_cache_reset();if let Some(c)=self.room_cache.borrow().calculations.get(&id){return Some(c.clone());}let r=self.room(id)?;
        let mut c=RoomCalculation{status:RoomStatus::NotPlaced,region:None,area_mm2:0.0,perimeter_mm:0.0,volume_mm3:None,height_mm:self.room_height(r)};
        if let Some(at)=r.at{c.status=RoomStatus::NotEnclosed;if let Some(region)=self.room_faces(r.level).iter().find(|f|contains(f,at)).cloned(){
            if self.rooms().iter().any(|(other,room)|*other<id&&room.level==r.level&&room.phase==r.phase&&room.at.is_some_and(|p|contains(&region,p))){c.status=RoomStatus::Redundant;}else{c.status=RoomStatus::Placed;c.area_mm2=region.area();c.perimeter_mm=std::iter::once(&region.outer).chain(&region.holes).map(|ring|ring.iter().enumerate().map(|(i,p)|p.dist(&ring[(i+1)%ring.len()])).sum::<f64>()).sum();if self.room_settings().volumes{c.volume_mm3=Some(self.room_volume(r,&region));}c.region=Some(region);}
        }}self.room_cache.borrow_mut().calculations.insert(id,c.clone());Some(c)
    }
    pub fn room_at(&self,level:Id,point:Point)->Option<Id>{let face=self.room_faces(level);let region=face.iter().find(|f|contains(f,point))?;self.rooms().into_iter().filter(|(_,r)|r.level==level&&r.at.is_some_and(|p|contains(region,p))).map(|(id,_)|id).min()}

    pub fn add_room(&mut self,level:Id,at:Option<Point>,view:Option<Id>)->Result<Id,String>{
        let base=self.level(level).ok_or("Unknown room level")?.elevation_mm;if at.is_some_and(|p|!p.x.is_finite()||!p.y.is_finite()){return Err("Invalid room location".into());}
        let upper=self.levels().into_iter().filter(|(_,l)|l.elevation_mm>base+0.1).min_by(|a,b|a.1.elevation_mm.total_cmp(&b.1.elevation_mm)).map(|(id,_)|id);
        let mut number=self.rooms().last().map(|(_,r)|r.number.clone()).unwrap_or("0".into());let start=number.char_indices().rev().take_while(|(_,c)|c.is_ascii_digit()).last().map(|(i,_)|i).unwrap_or(number.len());let prefix=number[..start].to_string();let width=number.len()-start;let mut n=number[start..].parse::<u64>().unwrap_or(0)+1;loop{number=format!("{prefix}{n:0width$}",width=width);if !self.rooms().iter().any(|(_,r)|r.number==number){break;}n+=1;}
        let r=Room{level,at,number,name:"Room".into(),upper_level:upper,limit_offset_mm:if upper.is_some(){0.0}else{default_height()},base_offset_mm:0.0,comments:String::new(),occupancy:String::new(),department:String::new(),occupant:String::new(),base_finish:String::new(),floor_finish:String::new(),wall_finish:String::new(),ceiling_finish:String::new(),phase:None,pinned:false,ifc:crate::element::IfcParams{guid:crate::element::new_ifc_guid(),..Default::default()}};
        self.grouped("Place room",|d|{let id=d.transact("Room",|tx|Ok(tx.add(Element::Room(r))))?;if let(Some(view),Some(at))=(view,at){d.add_room_tag(view,id,at)?;}Ok(id)})
    }
    pub fn patch_room(&mut self,id:Id,patch:&serde_json::Value)->Result<(),String>{let old=self.room(id).cloned().ok_or("not a room")?;let mut new:Room=crate::doc::apply_patch(&old,patch)?;new.level=old.level;if old.pinned&&new.at!=old.at{return Err("Unpin the room before moving it.".into());}if new.number.trim().is_empty()||new.name.trim().is_empty(){return Err("A room needs a name and number.".into());}if !new.limit_offset_mm.is_finite()||!new.base_offset_mm.is_finite()||self.room_height(&new)<=0.0{return Err("The room upper boundary must be above its base.".into());}if new.upper_level.is_some_and(|id|self.level(id).is_none())||new.at.is_some_and(|p|!p.x.is_finite()||!p.y.is_finite()){return Err("Invalid room boundary or location.".into());}self.transact("Room properties",|tx|tx.update(id,Element::Room(new)))}
    pub fn unplace_room(&mut self,id:Id)->Result<(),String>{let tags:Vec<_>=self.iter().filter_map(|(tid,e)|matches!(e,Element::RoomTag(t)if t.room==id).then_some(tid)).collect();self.grouped("Unplace room",|d|{d.patch_room(id,&serde_json::json!({"at":null}))?;d.transact("Remove room tags",|tx|{for tid in tags{tx.remove(tid)?;}Ok(())})})}
    pub fn place_unplaced_room(&mut self,id:Id,level:Id,at:Point)->Result<(),String>{let mut room=self.room(id).cloned().ok_or("not a room")?;if room.at.is_some(){return Err("That room is already placed.".into());}if self.level(level).is_none(){return Err("Unknown room level".into());}if !at.x.is_finite()||!at.y.is_finite(){return Err("Invalid room location".into());}let height=self.room_height(&room);room.level=level;if self.room_height(&room)<=0.0{room.upper_level=None;room.limit_offset_mm=height+room.base_offset_mm;}room.at=Some(at);self.transact("Place unplaced room",|tx|tx.update(id,Element::Room(room)))}
    pub fn add_room_separators(&mut self,level:Id,curves:Vec<Curve>)->Result<Vec<Id>,String>{if self.level(level).is_none(){return Err("Unknown level".into());}for c in &curves{let p=c.tessellate(1.5);if p.len()<2||p.iter().any(|p|!p.x.is_finite()||!p.y.is_finite())||c.length()<0.1{return Err("Invalid separation curve".into());}}self.transact("Room separation",|tx|Ok(curves.into_iter().map(|curve|tx.add(Element::RoomSeparation(RoomSeparation{level,curve,phase:None,pinned:false}))).collect()))}

    pub fn add_room_tag(&mut self,view:Id,room:Id,at:Point)->Result<Id,String>{if self.room(room).is_none()||self.view(view).is_none(){return Err("Unknown room or view".into());}let family=self.ensure_room_tag_family();self.transact("Tag room",|tx|Ok(tx.add(Element::RoomTag(RoomTag{view,room,at,family:Some(family),leader:false,pinned:false}))))}
    pub fn patch_room_tag(&mut self,id:Id,patch:&serde_json::Value)->Result<(),String>{let Some(Element::RoomTag(t))=self.get(id)else{return Err("not a room tag".into())};let old=t;let t:RoomTag=crate::doc::apply_patch(t,patch)?;if old.pinned&&t.at!=old.at{return Err("Unpin the tag before moving it.".into());}if !t.at.x.is_finite()||!t.at.y.is_finite()||t.family.is_some_and(|id|self.annotation_family(id).is_none_or(|f|f.kind!=crate::annotation::AnnotationKind::RoomTag)){return Err("Invalid room tag family or location.".into());}if self.room(t.room).is_none()||self.view(t.view).is_none(){return Err("Unknown room or view".into());}self.transact("Room tag properties",|tx|tx.update(id,Element::RoomTag(t)))}

    /// Room-aware family point, or the insertion point; doors/windows sample both sides of their host.
    pub fn instance_rooms(&self,id:Id)->(Option<Id>,Option<Id>,Option<Id>){let Some(Element::FamilyInstance(i))=self.get(id)else{return(None,None,None)};let Some(frame)=self.instance_frame(id)else{return(None,None,None)};let family=self.instance_family(i).map(|(f,_)|f);let settings=family.map(|f|&f.settings);let local=settings.filter(|s|s.room_calculation_point).map_or([0.0;3],|s|s.room_point);let p=frame.fam_point(local);let at=|p:[f64;3]|self.room_at(i.level,Point::new(p[0],p[1])).filter(|id|self.room(*id).is_some_and(|r|{let base=self.level(r.level).map_or(0.0,|l|l.elevation_mm)+r.base_offset_mm;p[2]>=base-0.1&&p[2]<=base+self.room_height(r)+0.1}));let room=at(p);if i.host.is_none(){return(room,None,None);}let span=frame.wall_left.abs().max(frame.wall_right.abs())+100.0;let from=settings.filter(|s|s.room_calculation_point).and_then(|s|s.room_from).map(|p|frame.fam_point(p)).unwrap_or_else(||{let p=frame.fam_world(0.0,-span);[p.x,p.y,frame.z_ref]});let to=settings.filter(|s|s.room_calculation_point).and_then(|s|s.room_to).map(|p|frame.fam_point(p)).unwrap_or_else(||{let p=frame.fam_world(0.0,span);[p.x,p.y,frame.z_ref]});(room,at(from),at(to)) }

    fn room_caps(&self)->Arc<Vec<[[f64;3];3]>>{self.room_cache_reset();if let Some(c)=&self.room_cache.borrow().caps{return c.clone();}let mut tris=vec![];for (_,e) in self.iter(){let mesh=match e{Element::Floor(f)if f.room_bounding=>crate::mesh::floor_mesh(self,f),Element::Roof(r)if r.room_bounding=>crate::mesh::roof_mesh(self,r),Element::Ceiling(c)if c.room_bounding=>crate::mesh::ceiling_mesh(self,c),_=>None};if let Some(mesh)=mesh{for t in mesh.chunks_exact(18){tris.push([[t[0]as f64*1000.0,t[1]as f64*1000.0,t[2]as f64*1000.0],[t[6]as f64*1000.0,t[7]as f64*1000.0,t[8]as f64*1000.0],[t[12]as f64*1000.0,t[13]as f64*1000.0,t[14]as f64*1000.0]]);}}}let tris=Arc::new(tris);self.room_cache.borrow_mut().caps=Some(tris.clone());tris}
    fn room_volume(&self,r:&Room,region:&Region)->f64{let base=self.level(r.level).map_or(0.0,|l|l.elevation_mm)+r.base_offset_mm;let top=base+self.room_height(r);let caps=self.room_caps();let height=|p:Point|{let cap=caps.iter().filter_map(|t|z_at(p,t)).filter(|z|*z>base+0.1).fold(top,f64::min);(cap-base).max(0.0)};let Some((pts,tris))=crate::geom::triangulate_region(region)else{return 0.0};tris.iter().map(|t|integrate([pts[t[0]],pts[t[1]],pts[t[2]]],&height,0)).sum()}
}

fn z_at(p:Point,t:&[[f64;3];3])->Option<f64>{let (a,b,c)=(t[0],t[1],t[2]);let den=(b[1]-c[1])*(a[0]-c[0])+(c[0]-b[0])*(a[1]-c[1]);if den.abs()<1e-9{return None;}let u=((b[1]-c[1])*(p.x-c[0])+(c[0]-b[0])*(p.y-c[1]))/den;let v=((c[1]-a[1])*(p.x-c[0])+(a[0]-c[0])*(p.y-c[1]))/den;let w=1.0-u-v;(u>=-1e-8&&v>=-1e-8&&w>=-1e-8).then_some(u*a[2]+v*b[2]+w*c[2])}
fn integrate(t:[Point;3],f:&impl Fn(Point)->f64,depth:u8)->f64{let mid=|a:Point,b:Point|Point::new((a.x+b.x)/2.0,(a.y+b.y)/2.0);let [a,b,c]=t;let (ab,bc,ca)=(mid(a,b),mid(b,c),mid(c,a));let centre=Point::new((a.x+b.x+c.x)/3.0,(a.y+b.y+c.y)/3.0);let area=crate::geom::polygon_area_abs(&t);let coarse=area*(f(a)+f(b)+f(c))/3.0;let fine=area*(f(ab)+f(bc)+f(ca))/3.0;if depth>=7||((fine-coarse).abs()<area*0.02&&(f(centre)-(f(a)+f(b)+f(c))/3.0).abs()<0.05){fine}else{[[a,ab,ca],[ab,b,bc],[ca,bc,c],[ab,bc,ca]].into_iter().map(|t|integrate(t,f,depth+1)).sum()}}

fn horizontal_rings(mesh:&[f32],z:f64)->Vec<Vec<Point>>{let mut segments=vec![];for t in mesh.chunks_exact(18){let p=[[t[0]as f64*1000.0,t[1]as f64*1000.0,t[2]as f64*1000.0],[t[6]as f64*1000.0,t[7]as f64*1000.0,t[8]as f64*1000.0],[t[12]as f64*1000.0,t[13]as f64*1000.0,t[14]as f64*1000.0]];let mut hits=vec![];for k in 0..3{let(a,b)=(p[k],p[(k+1)%3]);if(a[2]<=z&&b[2]>z)||(b[2]<=z&&a[2]>z){let f=(z-a[2])/(b[2]-a[2]);hits.push(Point::new(a[0]+f*(b[0]-a[0]),a[1]+f*(b[1]-a[1])));}}if hits.len()==2{segments.push((hits[0],hits[1]));}}let mut rings=vec![];while let Some((a,b))=segments.pop(){let mut ring=vec![a,b];while ring.last().unwrap().dist(&a)>0.02{let end=*ring.last().unwrap();let Some(i)=segments.iter().position(|(a,b)|a.dist(&end)<0.02||b.dist(&end)<0.02)else{break};let(x,y)=segments.swap_remove(i);ring.push(if x.dist(&end)<0.02{y}else{x});}if ring.len()>3&&ring.last().unwrap().dist(&a)<0.02{ring.pop();if crate::geom::signed_area(&ring)<0.0{ring.reverse();}rings.push(ring);}}rings}
