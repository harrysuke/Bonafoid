use std::fs;

use nocode_bpmn::xml;

fn main() {
    let bpmn_dir = r"D:\nocode-ide\bpmn";
    let mut ok = 0;
    let mut fail = 0;
    for entry in fs::read_dir(bpmn_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("bpmn") {
            let xml = fs::read_to_string(&path).unwrap();
            match xml::parse_bpmn(&xml, path.file_stem().unwrap().to_str().unwrap()) {
                Ok(w) => { println!("OK  {} => workflow '{}' nodes={} edges={}", path.display(), w.name, w.nodes.len(), w.edges.len()); ok += 1; }
                Err(e) => { println!("ERR {} => {}", path.display(), e); fail += 1; }
            }
        }
    }
    println!("Phase B import: {} OK, {} FAIL", ok, fail);
}
