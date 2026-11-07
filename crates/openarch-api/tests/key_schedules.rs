use openarch_api::Session;
use serde_json::{json, Value};

fn call(s: &mut Session, method: &str, params: Value) -> Value { s.handle(method, &params).unwrap_or_else(|e| panic!("{method}: {e}")) }
fn field(name: &str) -> Value { json!({"name":name,"kind":"instance"}) }
fn key(s: &mut Session, name: &str, fields: &[&str]) -> u64 {
    let id = call(s,"add_schedule",json!({"category":"Doors","name":name,"key":true,"key_name":name}))["id"].as_u64().unwrap();
    let mut def = call(s,"schedule_table",json!({"id":id}))["def"].clone();
    def["fields"] = json!(std::iter::once(field("Key Name")).chain(fields.iter().map(|n|field(n))).collect::<Vec<_>>());
    call(s,"set_schedule",json!({"id":id,"def":def})); id
}
fn doors(s: &mut Session) -> [u64;2] {
    let info=call(s,"doc_info",json!({}));
    let wall=call(s,"add_wall",json!({"level_id":info["levels"][0]["id"],"type_id":info["wall_types"][0]["id"],"start":[0,0],"end":[10000,0],"height_mm":3000}))["id"].as_u64().unwrap();
    let ty=info["family_types"].as_array().unwrap().iter().find(|t|t["label"]=="Single-Flush: 820 x 2040").unwrap()["id"].clone();
    [2000,6000].map(|x|call(s,"place_instance",json!({"type_id":ty,"host_id":wall,"point":[x,0]}))["id"].as_u64().unwrap())
}
fn assign(s:&mut Session,key:u64,ids:&[u64],row:Option<u64>) { call(s,"patch_element_keys",json!({"ids":ids,"patch":{"keys":{key.to_string():row}}})); }
fn prop(s:&mut Session,id:u64,name:&str)->Value { call(s,"describe",json!({"id":id}))["groups"].as_array().unwrap().iter().flat_map(|g|g["rows"].as_array().unwrap()).find(|r|r["label"]==name).unwrap().clone() }

#[test]
fn keys_propagate_lock_retain_values_and_undo_as_one_edit() {
    let mut s=Session::new();
    let parameter=call(&mut s,"add_project_parameter",json!({"category":"Doors","name":"Hardware Set","unit":"text"}))["id"].as_u64().unwrap();
    let k=key(&mut s,"Door Hardware",&["Hardware Set","Sill Height"]);
    let a=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap();
    call(&mut s,"patch_key_row",json!({"id":k,"row":a,"patch":{"name":"Office","values":{"Hardware Set":"H01","Sill Height":100}}}));
    let ids=doors(&mut s); assign(&mut s,k,&ids,Some(a));
    assert_eq!(s.doc.project_parameter(parameter).unwrap().values[&ids[0]],"H01");
    assert_eq!(prop(&mut s,ids[0],"Sill Height")["raw"],100.0);
    assert_eq!(prop(&mut s,ids[0],"Hardware Set")["kind"],"ro");
    assert!(s.handle("patch_instance",&json!({"id":ids[0],"patch":{"values":{"Sill Height":200}}})).is_err());
    assert!(s.doc.set_project_value(parameter,ids[0],json!("forbidden")).is_err());
    call(&mut s,"patch_key_row",json!({"id":k,"row":a,"patch":{"values":{"Hardware Set":"H02","Sill Height":150}}}));
    assert_eq!(prop(&mut s,ids[1],"Sill Height")["raw"],150.0);
    call(&mut s,"undo",json!({}));
    assert_eq!(prop(&mut s,ids[1],"Sill Height")["raw"],100.0);
    assert_eq!(s.doc.project_parameter(parameter).unwrap().values[&ids[0]],"H01");
    call(&mut s,"redo",json!({}));
    assign(&mut s,k,&[ids[0]],None);
    assert_eq!(prop(&mut s,ids[0],"Sill Height")["raw"],150.0);
    assert_eq!(prop(&mut s,ids[0],"Hardware Set")["kind"],"text");
    s.doc.set_project_value(parameter,ids[0],json!("Manual")).unwrap();
    call(&mut s,"delete_key_rows",json!({"id":k,"rows":[a]}));
    assert_eq!(prop(&mut s,ids[1],"Hardware Set")["value"],"H02");
    assert_eq!(prop(&mut s,ids[1],"Hardware Set")["kind"],"text");
    let b=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap(); assert!(b>a);
    assert_eq!(s.doc.schedule(k).unwrap().key.as_ref().unwrap().assignments.len(),0);
}

#[test]
fn invalid_updates_roll_back_every_assignment_and_key_value() {
    let mut s=Session::new(); let k=key(&mut s,"Door Positions",&["Sill Height"]);
    let row=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap();
    let ids=doors(&mut s); assign(&mut s,k,&ids,Some(row));
    let before=call(&mut s,"schedule_table",json!({"id":k}));
    assert!(s.handle("patch_key_row",&json!({"id":k,"row":row,"patch":{"values":{"Sill Height":9000}}})).is_err());
    assert_eq!(call(&mut s,"schedule_table",json!({"id":k})),before);
    assert_eq!(prop(&mut s,ids[0],"Sill Height")["raw"],0.0);
    assert!(s.handle("patch_key_row",&json!({"id":k,"row":row,"patch":{"values":{"Sill Height":"bad"}}})).is_err());
    let mut def=before["def"].clone();def["fields"].as_array_mut().unwrap().push(json!({"name":"Width","kind":"type"}));
    assert!(s.handle("set_schedule",&json!({"id":k,"def":def})).is_err());
    let other=call(&mut s,"add_schedule",json!({"category":"Doors","name":"Other","key":true,"key_name":"Other"}))["id"].as_u64().unwrap();
    let mut def=call(&mut s,"schedule_table",json!({"id":other}))["def"].clone();def["fields"].as_array_mut().unwrap().push(field("Sill Height"));
    assert!(s.handle("set_schedule",&json!({"id":other,"def":def})).is_err());
}

#[test]
fn ordinary_schedules_edit_keys_and_files_preserve_custom_parameters() {
    let mut s=Session::new();
    let parameter=call(&mut s,"add_project_parameter",json!({"category":"Doors","name":"Fire Rating","unit":"text","shared_guid":"11111111-1111-4111-8111-111111111111"}))["id"].as_u64().unwrap();
    let k=key(&mut s,"Fire Set",&["Fire Rating"]);let row=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap();
    call(&mut s,"patch_key_row",json!({"id":k,"row":row,"patch":{"name":"FRL 60","values":{"Fire Rating":"60/60/60"}}}));
    let ids=doors(&mut s);
    let normal=call(&mut s,"add_schedule",json!({"category":"Doors","name":"Door List"}))["id"].as_u64().unwrap();
    let mut def=call(&mut s,"schedule_table",json!({"id":normal}))["def"].clone();def["fields"]=json!([field("Fire Set"),field("Fire Rating")]);
    call(&mut s,"set_schedule",json!({"id":normal,"def":def}));
    let table=call(&mut s,"schedule_table",json!({"id":normal}));let cell=&table["rows"][0]["cells"][0];
    assert_eq!(cell["targets"][0]["method"],"patch_element_keys");
    let target=&cell["targets"][0];let options=cell["row"]["options"].as_array().unwrap();
    call(&mut s,"patch_element_keys",json!({"id":target["id"],"ids":target["extra"]["ids"],"patch":options.iter().find(|o|o["label"]=="FRL 60").unwrap()["patch"]}));
    assert_eq!(prop(&mut s,ids[0],"Fire Rating")["value"],"60/60/60");
    let path=std::env::temp_dir().join(format!("openarch-keys-{}.oab",std::process::id()));
    call(&mut s,"save",json!({"path":path}));call(&mut s,"open",json!({"path":path}));
    assert_eq!(s.doc.project_parameter(parameter).unwrap().shared_guid.as_deref(),Some("11111111-1111-4111-8111-111111111111"));
    assert_eq!(prop(&mut s,ids[0],"Fire Rating")["kind"],"ro");
    call(&mut s,"delete",json!({"ids":[k]}));
    assert_eq!(prop(&mut s,ids[0],"Fire Rating")["kind"],"text");
    assert_eq!(prop(&mut s,ids[0],"Fire Rating")["value"],"60/60/60");
    std::fs::remove_file(path).unwrap();
}

#[test]
fn copies_selector_renames_blank_values_and_deletion_keep_consistent_state() {
    let mut s=Session::new();
    let parameter=call(&mut s,"add_project_parameter",json!({"category":"Doors","name":"Hardware Code","unit":"text"}))["id"].as_u64().unwrap();
    let k=key(&mut s,"Hardware Style",&["Hardware Code"]);let row=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap();
    let ids=doors(&mut s);assign(&mut s,k,&ids,Some(row));
    assert!(s.doc.project_parameter(parameter).unwrap().values[&ids[0]].is_null(),"blank project values stay blank");
    call(&mut s,"patch_key_row",json!({"id":k,"row":row,"patch":{"values":{"Hardware Code":"HW"}}}));
    let copy=call(&mut s,"transform",json!({"ids":[ids[0]],"kind":"move","copy":true,"dx":1200,"dy":0}))["ids"][0].as_u64().unwrap();
    assert_eq!(s.doc.schedule(k).unwrap().key.as_ref().unwrap().assignments[&copy],row);
    assert_eq!(prop(&mut s,copy,"Hardware Code")["value"],"HW");
    assert_eq!(prop(&mut s,copy,"Hardware Code")["kind"],"ro");
    let normal=call(&mut s,"add_schedule",json!({"category":"Doors","name":"Door List"}))["id"].as_u64().unwrap();
    let mut def=call(&mut s,"schedule_table",json!({"id":normal}))["def"].clone();def["fields"]=json!([field("Hardware Style")]);
    call(&mut s,"set_schedule",json!({"id":normal,"def":def}));
    call(&mut s,"patch_schedule",json!({"id":k,"patch":{"key":{"parameter_name":"Door Hardware"}}}));
    assert_eq!(s.doc.schedule(normal).unwrap().fields[0].name,"Door Hardware");
    assert_eq!(prop(&mut s,copy,"Door Hardware")["value"],"1");
    call(&mut s,"delete",json!({"ids":[copy]}));
    assert!(!s.doc.schedule(k).unwrap().key.as_ref().unwrap().assignments.contains_key(&copy));
    assert!(!s.doc.project_parameter(parameter).unwrap().values.contains_key(&copy));
    call(&mut s,"undo",json!({}));
    assert_eq!(s.doc.schedule(k).unwrap().key.as_ref().unwrap().assignments[&copy],row);
    assert_eq!(s.doc.project_parameter(parameter).unwrap().values[&copy],"HW");
}

#[test]
fn shared_family_dimensions_drive_geometry_and_blank_keys_use_type_defaults() {
    let mut s=Session::new();let ids=doors(&mut s);
    let instance=s.doc.instances().into_iter().find(|(id,_)|*id==ids[0]).unwrap().1;
    let original_opening=s.doc.instance_opening_size(instance).0;
    let family=s.doc.family_type(instance.type_id).unwrap().family;
    let mut params=s.doc.family(family).unwrap().params.clone();
    let p=params.iter_mut().find(|p|p.name=="Width").unwrap();p.instance=true;p.shared_guid=Some("33333333-3333-4333-8333-333333333333".into());
    s.doc.set_family_def(family,Some(params),None).unwrap();
    let k=key(&mut s,"Opening Style",&["Width"]);let row=call(&mut s,"add_key_row",json!({"id":k}))["row"].as_u64().unwrap();
    call(&mut s,"patch_key_row",json!({"id":k,"row":row,"patch":{"values":{"Width":950}}}));assign(&mut s,k,&ids,Some(row));
    for id in ids { let i=s.doc.instances().into_iter().find(|(eid,_)|*eid==id).unwrap().1;assert_eq!(s.doc.instance_param(i,"Width").unwrap().as_f64(),Some(950.0));assert!((s.doc.instance_opening_size(i).0-original_opening-130.0).abs()<1e-6); }
    assert_eq!(s.doc.schedule(k).unwrap().key.as_ref().unwrap().bindings["Width"].shared_guid.as_deref(),Some("33333333-3333-4333-8333-333333333333"));
    call(&mut s,"patch_key_row",json!({"id":k,"row":row,"patch":{"values":{"Width":null}}}));
    let i=s.doc.instances().into_iter().find(|(id,_)|*id==ids[0]).unwrap().1;
    assert!((s.doc.instance_opening_size(i).0-original_opening).abs()<1e-6);
}

#[test]
fn railings_ceilings_stairs_ramps_and_areas_take_project_parameters_and_keys() {
    let mut s = Session::new();
    let info = call(&mut s, "doc_info", json!({}));
    let level = info["levels"][0]["id"].clone();
    let rail = info["railing_types"][0]["id"].clone();
    let curves = |y: i64| json!([{ "type": "line", "a": { "x": 0, "y": y }, "b": { "x": 3000, "y": y } }]);
    let ids: Vec<u64> = [0, 2000].iter().map(|y| call(&mut s, "add_railing", json!({ "type_id": rail, "level_id": level, "curves": curves(*y) }))["id"].as_u64().unwrap()).collect();
    // a project parameter shows in a railing's Properties and takes a value
    let parameter = call(&mut s, "add_project_parameter", json!({ "category": "Railings", "name": "Balustrade Ref", "unit": "text" }))["id"].as_u64().unwrap();
    assert!(prop(&mut s, ids[0], "Balustrade Ref")["kind"] != "ro");
    // a key schedule of railings sets Comments and the project parameter on both
    let k = call(&mut s, "add_schedule", json!({ "category": "Railings", "name": "Balustrades", "key": true, "key_name": "Balustrade Style" }))["id"].as_u64().unwrap();
    let mut def = call(&mut s, "schedule_table", json!({ "id": k }))["def"].clone();
    def["fields"] = json!([field("Key Name"), field("Comments"), field("Balustrade Ref")]);
    call(&mut s, "set_schedule", json!({ "id": k, "def": def }));
    let row = call(&mut s, "add_key_row", json!({ "id": k }))["row"].as_u64().unwrap();
    call(&mut s, "patch_key_row", json!({ "id": k, "row": row, "patch": { "name": "Glass", "values": { "Comments": "Frameless glass", "Balustrade Ref": "B02" } } }));
    assign(&mut s, k, &ids, Some(row));
    for id in &ids {
        assert_eq!(prop(&mut s, *id, "Comments")["value"], "Frameless glass");
        assert_eq!(prop(&mut s, *id, "Comments")["kind"], "ro");
        assert_eq!(s.doc.project_parameter(parameter).unwrap().values[id], "B02");
    }
    assert!(s.handle("patch_railing", &json!({ "id": ids[0], "patch": { "comments": "other" } })).is_err());
    // the other new categories take parameters too, an area scheme as its own category
    for c in ["Ceilings", "Stairs", "Ramps"] {
        call(&mut s, "add_project_parameter", json!({ "category": c, "name": "Spec Ref", "unit": "text" }));
    }
    let scheme = s.doc.area_schemes()[0].1.name.clone();
    call(&mut s, "add_project_parameter", json!({ "category": format!("Areas ({scheme})"), "name": "Lease Ref", "unit": "text" }));
    assert!(s.handle("add_project_parameter", &json!({ "category": "Areas (No Such Scheme)", "name": "X", "unit": "text" })).is_err());
}
