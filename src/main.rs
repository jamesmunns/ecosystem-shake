use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use mvdb::Mvdb;
use serde::{Serialize, Deserialize};
use serde_json;
use walkdir::{WalkDir, Error as WdError};


#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Normal,
    Build,
    Dev,
}

#[derive(Serialize, Deserialize, Debug)]
struct Dependency {
    name: String,
    req: String,
    features: Vec<String>,
    optional: bool,
    default_features: bool,
    package: Option<String>,
    // target: ?
    kind: Option<Kind>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Krate {
    name: String,
    vers: String,
    deps: Vec<Dependency>,
    cksum: String,
    features: HashMap<String, Vec<String>>,
    yanked: bool,
    // links: Option<String>
}



#[derive(Serialize, Deserialize, Debug, Default)]
struct Database {
    crates: HashMap<String, HashMap<String, Krate>>,
    reverse_deps: HashMap<String, HashSet<String>>,
}


fn main() -> Result<(), WdError> {

    let mut index = Database::default();

    println!("Building forward deps tree...");

    let walker = WalkDir::new("crates.io-index")
        .into_iter()
        .filter_entry(|e| e.file_name().to_str().map(|s| s != ".git").unwrap_or(false));
    for entry in walker.filter(|w| w.as_ref().unwrap().path().is_file()) {
        // println!("{}", entry.as_ref().unwrap().path().display());
        let path = entry.as_ref().unwrap().path();

        if (&format!("{}", path.display())).ends_with(".json") {
            continue
        }
        let file = File::open(path).unwrap();
        for line in BufReader::new(file).lines() {
            let x: Krate = serde_json::from_str(&line.unwrap()).unwrap();

            if !index.crates.contains_key(&x.name) {
                index.crates.insert(x.name.clone(), HashMap::new());
            }

            index
                .crates
                .get_mut(&x.name)
                .unwrap()
                .insert(x.vers.clone(), x);
        }
    }

    println!("Building reverse deps tree...");

    // initialize with all empty sets
    let mut rindex: HashMap<String, HashSet<String>> = index
        .crates
        .keys()
        .map(|k| (k.to_string(), HashSet::new()))
        .collect();

    for (cr_ident, cr_vers) in index.crates.iter() {

        // TODO: store each version independently?
        for (_vers, cr_data) in cr_vers {
            for dep in cr_data.deps.iter() {

                if let Some(ref kind) = dep.kind {
                    if kind != &Kind::Normal {
                        continue;
                    }
                }

                let name = if let Some(pkg) = &dep.package {
                    pkg.clone()
                } else {
                    dep.name.clone()
                };

                if !rindex.contains_key(&name) {
                    println!("XXX {}", name);
                    rindex.insert(name.clone(), HashSet::new());
                }

                rindex
                    .get_mut(&name)
                    .unwrap()
                    .insert(cr_ident.clone());
            }
        }
    }

    index.reverse_deps = rindex;

    // let file = Path::new("index.json");
    // let my_data: Mvdb<Database> = Mvdb::from_file_or_default_pretty(&file).unwrap();
    // my_data.access_mut(|db| *db = index).unwrap();

    println!("Building embedded tree...");

    let seed_crates = ["cortex-m"];
    let mut todo_crates = seed_crates
        .iter()
        .map(|sr| sr.to_string())
        .collect::<Vec<String>>();

    let mut emb_index: HashSet<String> = HashSet::new();
    let mut maybe_respider: HashSet<String> = HashSet::new();

    // First pass - spider upwards
    while let Some(cr) = todo_crates.pop() {
        emb_index.insert(cr.to_string());
        println!("{}", cr);
        // let y: () = index.reverse_deps.get(cr)
        if let Some(rdeps) = index.reverse_deps.get(&cr) {
            for rdep in rdeps.iter() {
                if !emb_index.contains(rdep) {
                    println!("  -> {}", rdep);
                    todo_crates.push(rdep.to_string());
                } else {
                    println!("  -x {}", rdep);
                }
            }
        } else {
            println!("{} has no rdeps?!?!", cr);
        }
    }

    // Second pass - spider downwards
    let mut todo_crates = emb_index
        .iter()
        .map(|sr| sr.to_string())
        .collect::<Vec<String>>();

    while let Some(cr) = todo_crates.pop() {
        for (_ver, cr_info) in index.crates.get(&cr).unwrap().iter() {
            for dep in cr_info.deps.iter() {
                if let Some(ref kind) = dep.kind {
                    if kind != &Kind::Normal {
                        continue;
                    }
                }


                let name = if let Some(pkg) = &dep.package {
                    pkg.clone()
                } else {
                    dep.name.clone()
                };

                if !emb_index.contains(&name) {
                    maybe_respider.insert(name.clone());
                }

                emb_index.insert(name);
            }
        }
    }

    let mut idx_sz = 0;

    while idx_sz != emb_index.len() {
        idx_sz = emb_index.len();

        // Second pass - spider downwards
        let mut todo_crates = emb_index
            .iter()
            .map(|sr| sr.to_string())
            .collect::<Vec<String>>();

        while let Some(cr) = todo_crates.pop() {
            for (_ver, cr_info) in index.crates.get(&cr).unwrap().iter() {
                for dep in cr_info.deps.iter() {
                    if let Some(ref kind) = dep.kind {
                        if kind != &Kind::Normal {
                            continue;
                        }
                    }


                    let name = if let Some(pkg) = &dep.package {
                        pkg.clone()
                    } else {
                        dep.name.clone()
                    };

                    if !emb_index.contains(&name) {
                        maybe_respider.insert(name.clone());
                    }

                    emb_index.insert(name);
                }
            }
        }

        for cr in emb_index.iter() {
            let _ = maybe_respider.remove(cr);
        }


    }


    println!("emb_index = {:#?}", emb_index);
    println!("maybe_respider = {:#?}", maybe_respider);
    println!("emb_index.len() = {}", emb_index.len());

    Ok(())

}
